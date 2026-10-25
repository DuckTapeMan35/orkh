use nix::unistd::User;
use notify::{event::ModifyKind, recommended_watcher, EventKind, RecursiveMode, Watcher};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use yaml_rust2::YamlLoader;

#[derive(Clone)]
pub struct ConfigWatcher {
    config_tx: broadcast::Sender<Option<yaml_rust2::Yaml>>,
    workspace_tx: broadcast::Sender<Vec<Value>>,
    latest_config: Arc<Mutex<Option<yaml_rust2::Yaml>>>,
    latest_workspace: Arc<Mutex<Option<Vec<Value>>>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
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

        let latest_config: Arc<Mutex<Option<yaml_rust2::Yaml>>> = Arc::new(Mutex::new(None));
        let latest_workspace: Arc<Mutex<Option<Vec<Value>>>> = Arc::new(Mutex::new(None));

        // Workspace states from duckwm, for the whole run: it reconnects by itself.
        let socket = PathBuf::from(format!("/run/user/{}/duckwm.sock", get_user_uid()));
        tokio::spawn(crate::config::duckwm::monitor_workspaces(
            socket,
            workspace_tx.clone(),
            latest_workspace.clone(),
        ));

        // Load the initial config in a blocking task
        let config_path = config_path();
        let config_tx_init = config_tx.clone();
        let latest_config_init = latest_config.clone();
        tokio::spawn(async move {
            let initial_config = if config_path.exists() {
                tokio::task::spawn_blocking(move || Self::load_config_from_file(&config_path).ok())
                    .await
                    .unwrap_or(None)
            } else {
                None
            };
            *latest_config_init.lock().unwrap() = initial_config.clone();
            let _ = config_tx_init.send(initial_config);
        });

        // Watch the config for changes
        tokio::spawn(Self::watch_task(config_tx.clone(), latest_config.clone()));

        Self {
            config_tx,
            workspace_tx,
            latest_config,
            latest_workspace,
        }
    }

    /// The latest workspace states: one per number key, 1-9 then 0.
    pub fn get_workspace_states(&self) -> Vec<Value> {
        self.latest_workspace
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_default()
    }

    async fn watch_task(
        config_tx: broadcast::Sender<Option<yaml_rust2::Yaml>>,
        latest_config: Arc<Mutex<Option<yaml_rust2::Yaml>>>,
    ) {
        use std::sync::mpsc;
        use std::time::{Duration, Instant};

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

        std::thread::spawn(move || {
            // Create watchers
            let mut watcher = recommended_watcher(move |res| {
                let _ = fs_tx.send(res);
            })
            .expect("Failed to create watcher");

            // Watch only the specific files, not the directories
            watcher
                .watch(&config_symlink_thread, RecursiveMode::NonRecursive)
                .unwrap();

            // If the symlink target is different, watch it too
            if config_target_thread != config_symlink_thread {
                watcher
                    .watch(&config_target_thread, RecursiveMode::NonRecursive)
                    .unwrap();
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
                        let matches = event
                            .paths
                            .iter()
                            .any(|p| p == &config_target || p == &config_symlink);

                        // Check if config changed
                        if matches
                            && matches!(
                                event.kind,
                                EventKind::Modify(ModifyKind::Data(_))
                                    | EventKind::Modify(ModifyKind::Any)
                            )
                            && last_config.elapsed() > Duration::from_millis(200)
                        {
                            match Self::load_config_from_file(&config_target) {
                                Ok(config) => {
                                    *latest_config.lock().unwrap() = Some(config.clone());
                                    let _ = config_tx.send(Some(config.clone()));
                                }
                                Err(_e) => {}
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

    fn load_config_from_file(
        path: &PathBuf,
    ) -> Result<yaml_rust2::Yaml, Box<dyn std::error::Error>> {
        let contents = fs::read_to_string(path)?;
        let docs = YamlLoader::load_from_str(&contents)?;
        docs.into_iter()
            .next()
            .ok_or_else(|| "Empty YAML document".into())
    }

    /// Get a receiver for config updates
    pub fn subscribe_config(&self) -> broadcast::Receiver<Option<yaml_rust2::Yaml>> {
        self.config_tx.subscribe()
    }

    /// Get a receiver for workspace updates (raw output from window manager)
    pub fn subscribe_workspaces(&self) -> broadcast::Receiver<Vec<Value>> {
        self.workspace_tx.subscribe()
    }

    /// Get current config (for initialization)
    pub fn get_config(&self) -> Option<yaml_rust2::Yaml> {
        self.latest_config.lock().unwrap().clone()
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
    let symlink = PathBuf::from(get_home()).join(".config/orkh/config.yaml");
    std::fs::canonicalize(&symlink).unwrap_or(symlink)
}
