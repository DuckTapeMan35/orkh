use notify::{recommended_watcher, RecursiveMode, Watcher, EventKind, event::ModifyKind};
use crate::config::parse_mmsg_output;
use std::fs;
use std::path::PathBuf;
use tokio::sync::broadcast;
use yaml_rust2::YamlLoader;
use tokio::sync::broadcast::error::TryRecvError;
use serde::Deserialize;
use std::collections::HashMap;
use nix::unistd::User;

#[derive(Clone)]
pub struct ConfigWatcher {
    config_tx: broadcast::Sender<Option<yaml_rust2::Yaml>>,
    workspace_tx: broadcast::Sender<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub window_manager: String,
    pub log_level: String,
    pub key_positions: HashMap<String, KeyPosition>,
    pub modes: HashMap<String, Mode>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum KeyPosition {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct Mode {
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub keys: Vec<String>,
    pub color: ColorSpec,
    #[serde(default)]
    pub condition: Option<Condition>,
    #[serde(default)]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum Condition {
    WorkSpaces,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub enum Value {
    Active,
    Inactive,
    Focused,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ColorSpec {
    Rgb([u8; 3]),
}

impl ConfigWatcher {
    pub fn start() -> Self {
        let (config_tx, _) = broadcast::channel(16);
        let (workspace_tx, _) = broadcast::channel(16);
        
        // Clone senders for the watcher task
        let config_tx_watcher = config_tx.clone();
        let workspace_tx_watcher = workspace_tx.clone();
        
        // Initialize with current values
        let config_path = config_path();
        
        // Load initial values in a blocking task
        let config_tx_init = config_tx.clone();
        let workspace_tx_init = workspace_tx.clone();
        let config_path_clone = config_path.clone();
        
        tokio::spawn(async move {
            let initial_config = if config_path_clone.exists() {
                tokio::task::spawn_blocking(move || Self::load_config_from_file(&config_path_clone).ok())
                    .await
                    .unwrap_or(None)
            } else {
                None
            };
            
            let _ = config_tx_init.send(initial_config.clone());
            
            // Start workspace monitoring if we have config
            if let Some(config) = initial_config {
                // Extract window_manager string and clone it
                if let Some(window_manager) = config["window_manager"].as_str() {
                    let window_manager_string = window_manager.to_string();
                    
                    // Start workspace monitoring task
                    let workspace_tx_task = workspace_tx_init.clone();
                    tokio::spawn(async move {
                        Self::monitor_workspaces(workspace_tx_task, window_manager_string).await;
                    });
                }
            }
        });
        
        // Start watcher task
        tokio::spawn(async move {
            Self::watch_task(config_tx_watcher, workspace_tx_watcher).await;
        });
        
        Self {
            config_tx,
            workspace_tx,
        }
    }

    pub fn get_workspace_states(&self) -> Vec<Value> {
        if let Some(output) = self.get_recent_workspace_output() {
            let lines: Vec<&str> = output.lines().collect();
            parse_mmsg_output(&lines)
        } else {
            Vec::new()
        }
    }
    
    async fn monitor_workspaces(
        workspace_tx: broadcast::Sender<String>,
        window_manager: String,
    ) {
        use tokio::time::{interval, Duration};
        use std::process::Command;
        
        let mut interval_timer = interval(Duration::from_millis(100));
        
        loop {
            interval_timer.tick().await;
            
            // Get the appropriate command for the window manager
            let command = Self::get_wm_command(&window_manager);
            
            // Run the command in a blocking task
            let tx = workspace_tx.clone();
            let cmd_string = command.to_string();
            
            tokio::task::spawn_blocking(move || {
                if let Ok(output) = Command::new("su")
                    .arg("-")
                    .arg(get_user())
                    .arg("-c")
                    .arg(&cmd_string)
                    .output()
                {
                    if output.status.success() {
                        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                        let _ = tx.send(stdout);
                    } else {
                        let _stderr = String::from_utf8_lossy(&output.stderr).to_string();
                        // Error message removed
                    }
                }
            }).await.ok();
        }
    }
    
    fn get_wm_command(window_manager: &str) -> String {
        let user_uid = get_user_uid();
        match window_manager {
            "mangowc" => format!("XDG_RUNTIME_DIR=/run/user/{user_uid} mmsg -gt"),
            "mango" => format!("XDG_RUNTIME_DIR=/run/user/{user_uid} mmsg -gt"),
            "mangowm" => format!("XDG_RUNTIME_DIR=/run/user/{user_uid} mmsg -gt"),
            _ => {
                // Error message removed, default to mmsg
                format!("XDG_RUNTIME_DIR=/run/user/{user_uid} mmsg -gt")
            }
        }
    }
    
    async fn watch_task(
        config_tx: broadcast::Sender<Option<yaml_rust2::Yaml>>,
        workspace_tx: broadcast::Sender<String>,
    ) {
        use std::sync::mpsc;
        use std::time::{Instant, Duration};

        let (fs_tx, fs_rx) = mpsc::channel();

        // File system watcher needs to run in blocking thread
        let config_symlink = PathBuf::from(get_home()).join(".config/orkh/config.yaml");

        // Try to resolve the symlink
        let config_target = match std::fs::read_link(&config_symlink) {
            Ok(target) => {
                // If target is relative, make it absolute
                if target.is_absolute() {
                    target
                } else {
                    config_symlink.parent().unwrap().join(target)
                }
            }
            Err(_) => config_symlink.clone(), // Not a symlink, use the original path
        };

        // Clone paths for the thread
        let config_target_thread = config_target.clone();
        let config_symlink_thread = config_symlink.clone();

        // Store active workspace monitoring task
        let mut workspace_monitor_handle: Option<tokio::task::JoinHandle<()>> = None;
        let workspace_tx_handle = workspace_tx.clone();

        std::thread::spawn(move || {
            // Create watchers
            let mut watcher = recommended_watcher(move |res| {
                let _ = fs_tx.send(res);
            }).expect("Failed to create watcher");

            // Watch only the specific files, not the directories
            watcher.watch(&config_symlink_thread, RecursiveMode::NonRecursive).unwrap();

            // If the symlink target is different, watch it too
            if config_target_thread != config_symlink_thread {
                watcher.watch(&config_target_thread, RecursiveMode::NonRecursive).unwrap();
            }

            // Keep the watcher alive
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        });

        // Process file system events
        let mut last_config = Instant::now();

        // Use a blocking task to receive from the sync channel
        tokio::task::spawn_blocking(move || {
            while let Ok(event) = fs_rx.recv() {
                match event {
                    Ok(event) => {
                        let matches = event.paths.iter().any(|p| {
                            p == &config_target || p == &config_symlink
                        });

                        // Check if config changed
                        if matches
                            && matches!(event.kind, EventKind::Modify(ModifyKind::Data(_)) | EventKind::Modify(ModifyKind::Any))
                            && last_config.elapsed() > Duration::from_millis(200) {

                            match Self::load_config_from_file(&config_target) {
                                Ok(config) => {
                                    let _ = config_tx.send(Some(config.clone()));

                                    // Restart workspace monitoring if window manager changed
                                    if let Some(window_manager) = config["window_manager"].as_str() {
                                        let window_manager_string = window_manager.to_string();

                                        if let Some(handle) = workspace_monitor_handle.take() {
                                            handle.abort();
                                        }

                                        let tx = workspace_tx_handle.clone();
                                        workspace_monitor_handle = Some(tokio::spawn(async move {
                                            Self::monitor_workspaces(tx, window_manager_string).await;
                                        }));
                                    }
                                }
                                Err(_e) => {
                                    // Error message removed
                                }
                            }
                            last_config = Instant::now();
                        }
                    }
                    Err(_e) => {
                        // Error message removed
                    }
                }
            }
        });
    }

    fn load_config_from_file(path: &PathBuf) -> Result<yaml_rust2::Yaml, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let docs = YamlLoader::load_from_str(&contents)?;
        docs.into_iter().next()
            .ok_or_else(|| "Empty YAML document".into())
    }

    /// Get a receiver for config updates
    pub fn subscribe_config(&self) -> broadcast::Receiver<Option<yaml_rust2::Yaml>> {
        self.config_tx.subscribe()
    }

    /// Get a receiver for workspace updates (raw output from window manager)
    pub fn subscribe_workspaces(&self) -> broadcast::Receiver<String> {
        self.workspace_tx.subscribe()
    }

    /// Get current config (for initialization)
    pub fn get_config(&self) -> Option<yaml_rust2::Yaml> {
        let mut receiver = self.subscribe_config();
        match receiver.try_recv() {
            Ok(config) => config,
            Err(TryRecvError::Empty) => {
                // Return the latest value
                let mut latest = None;
                while let Ok(config) = receiver.try_recv() {
                    latest = config;
                }
                latest
            }
            Err(_) => None,
        }
    }

    /// Get recent workspace outputs (for initialization)
    pub fn get_recent_workspace_output(&self) -> Option<String> {
        let mut receiver = self.subscribe_workspaces();
        let mut latest = None;

        // Get the most recent workspace output
        while let Ok(output) = receiver.try_recv() {
            latest = Some(output);
        }

        latest
    }
}

fn get_user() -> String {
    std::env::var("ORKH_USER").expect("ORKH_USER env variable not set")
}

fn get_user_uid() -> u32 {
    let user_name = get_user();
    let user_info = User::from_name(user_name.as_str())
    .expect("getpwnam failed")
    .expect("user not found");
    user_info.uid.as_raw()
}

fn get_home() -> String {
    format!(
        "/home/{}",
        std::env::var("ORKH_USER").expect("ORKH_USER env variable not set")
    )
}

fn config_path() -> PathBuf {
    let symlink  =  PathBuf::from(get_home()).join(".config/orkh/config.yaml");
    std::fs::canonicalize(&symlink).unwrap_or(symlink)
}
