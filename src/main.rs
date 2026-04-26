use openrgb2::{Color, OpenRgbClient, OpenRgbResult};
use orkh::keyboard::KeyboardListener;
use orkh::config::{ConfigWatcher, parse_config, LedApplicator, parse_mmsg_output};
use std::collections::HashSet;
use std::process::Command;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> OpenRgbResult<()> {
    // Spawn OpenRGB server asynchronously
    let mut server = Command::new("openrgb")
        .arg("--server")
        .spawn()?;

    // Sleep for 5 seconds for openrgb to fully initialize
    sleep(Duration::from_secs(5)).await;

    // Start keyboard listener
    let keyboard = KeyboardListener::start();
    let watcher = ConfigWatcher::start();
    
    // Get receivers for updates
    let mut config_rx = watcher.subscribe_config();
    let mut workspace_rx = watcher.subscribe_workspaces(); // Add workspace receiver
    
    // Current state
    let mut config_yaml = watcher.get_config();
    let mut config = config_yaml.as_ref().and_then(parse_config);
    let mut applicator = config.as_ref().map(LedApplicator::new);
    
    // Get initial workspace states
    let mut workspace_states = if let Some(workspace_output) = watcher.get_recent_workspace_output() {
        let lines: Vec<&str> = workspace_output.lines().collect();
        parse_mmsg_output(&lines)
    } else {
        Vec::new()
    };
    
    // Update applicator with initial workspace states
    if let Some(ref mut app) = applicator.as_mut() {
        app.update_workspace_states(workspace_states.clone());
    }

    // Connect to OpenRGB
    let client = OpenRgbClient::connect().await?;

    // Optional: if you want the server to run in background
    tokio::spawn(async move {
        let _ = server.wait();
    });

    let controller = client
        .get_all_controllers()
        .await?
        .into_iter()
        .next()
        .expect("No controllers found");

    controller.init().await?;
    let zone = controller.get_zone(0)?;
    
    // Function to build modes from config
    fn build_modes_from_config(config: &Option<orkh::config::Config>) -> HashSet<String> {
        match config {
            Some(cfg) => {
                // Get all modes defined in config
                cfg.modes.keys().cloned().collect::<HashSet<_>>()
            }
            None => {
                // Default modes if no config
                [
                    "super", "shift", "ctrl", "alt",
                    "super_shift", "super_ctrl", "super_alt",
                    "shift_ctrl", "shift_alt", "ctrl_alt",
                ]
                .into_iter()
                .map(String::from)
                .collect()
            }
        }
    }

    let mut modes = build_modes_from_config(&config);
    // Note: get_current_mode is now async
    let mut prev_mode = keyboard.state.get_current_mode(&modes).await;
    
    // Apply initial mode
    if let (Some(cfg), Some(app)) = (config.as_ref(), applicator.as_ref()) {
        let mut cmd = controller.cmd();
        if let Err(e) = app.apply_mode(&prev_mode, cfg, &mut cmd, &zone) {
            eprintln!("Error applying initial mode: {}", e);
        }
        cmd.execute().await?;
    }
    
    // Use a longer polling interval for keyboard (100ms is usually sufficient)
    let mut keyboard_interval = tokio::time::interval(tokio::time::Duration::from_millis(100));
    keyboard_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    
    loop {
        tokio::select! {
            // Config changed signal
            Ok(new_config_yaml) = config_rx.recv() => {
                config_yaml = new_config_yaml;
                config = config_yaml.as_ref().and_then(parse_config);
                if let Some(ref cfg) = config {
                    applicator = Some(LedApplicator::new(cfg));
                    // Update applicator with current workspace states
                    if let Some(ref mut app) = applicator.as_mut() {
                        app.update_workspace_states(workspace_states.clone());
                    }
                    // Rebuild modes from new config
                    modes = build_modes_from_config(&config);
                }
                
                // Note: get_current_mode is now async
                let curr_mode = keyboard.state.get_current_mode(&modes).await;
                
                if let (Some(cfg), Some(app)) = (config.as_ref(), applicator.as_ref()) {
                    let mut cmd = controller.cmd();
                    if let Err(e) = app.apply_mode(&curr_mode, cfg, &mut cmd, &zone) {
                        eprintln!("Error applying mode after config update: {}", e);
                    }
                    cmd.execute().await?;
                }
            }
            
            // Workspace state changed
            Ok(workspace_output) = workspace_rx.recv() => {
                // Parse the workspace output
                let lines: Vec<&str> = workspace_output.lines().collect();
                let new_states = parse_mmsg_output(&lines);
                
                // Update workspace states
                workspace_states = new_states;
                
                // Update applicator
                if let Some(ref mut app) = applicator.as_mut() {
                    app.update_workspace_states(workspace_states.clone());
                }
                
                // Note: get_current_mode is now async
                let curr_mode = keyboard.state.get_current_mode(&modes).await;
                
                if let (Some(cfg), Some(app)) = (config.as_ref(), applicator.as_ref()) {
                    let mut cmd = controller.cmd();
                    if let Err(e) = app.apply_mode(&curr_mode, cfg, &mut cmd, &zone) {
                        eprintln!("Error applying mode after workspace update: {}", e);
                    }
                    cmd.execute().await?;
                }
            }
            
            // Keyboard state check
            _ = keyboard_interval.tick() => {
                // Note: get_current_mode is now async
                let curr_mode = keyboard.state.get_current_mode(&modes).await;
                
                if curr_mode != prev_mode {
                    if let (Some(cfg), Some(app)) = (config.as_ref(), applicator.as_ref()) {
                        let mut cmd = controller.cmd();
                        
                        if let Err(e) = app.apply_mode(&curr_mode, cfg, &mut cmd, &zone) {
                            eprintln!("Error applying mode: {}", e);
                        }
                        
                        cmd.execute().await?;
                    } else {
                        // Fallback: just turn all LEDs off
                        let mut cmd = controller.cmd();
                        for led in zone.led_iter() {
                            cmd.set_led(led.id(), Color::new(0, 0, 0))?;
                        }
                        cmd.execute().await?;
                    }
                    prev_mode = curr_mode;
                }
            }
        }
    }
}
