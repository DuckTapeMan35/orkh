use openrgb2::{Color, OpenRgbClient, OpenRgbResult};
use orkh::config::{parse_config, ConfigWatcher, LedApplicator};
use orkh::keyboard::KeyboardListener;
use std::collections::HashSet;
use std::process::Command;
use tokio::time::{sleep, Duration};

const OPENRGB: &str = match option_env!("ORKH_OPENRGB") {
    Some(p) => p,
    None => "openrgb",
};

fn main() -> OpenRgbResult<()> {
    orkh::sandbox::ensure_sandboxed();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> OpenRgbResult<()> {
    // Spawn OpenRGB server asynchronously
    let mut server = Command::new(OPENRGB).arg("--server").spawn()?;

    // Sleep for 5 seconds for openrgb to fully initialize
    sleep(Duration::from_secs(5)).await;

    // Start keyboard listener
    let keyboard = KeyboardListener::start();
    let watcher = ConfigWatcher::start();

    // Get receivers for updates
    let mut config_rx = watcher.subscribe_config();
    let mut workspace_rx = watcher.subscribe_workspaces();
    let mut keybind_rx = watcher.subscribe_keybinds();

    // Current state
    let mut config_yaml = watcher.get_config();
    let mut config = config_yaml.as_ref().and_then(parse_config);
    let mut applicator = config.as_ref().map(LedApplicator::new);

    // Get initial workspace states and keybinds
    let mut workspace_states = watcher.get_workspace_states();
    let mut keybinds = watcher.get_keybinds();

    // Update applicator with them
    if let (Some(cfg), Some(app)) = (config.as_ref(), applicator.as_mut()) {
        app.update_workspace_states(workspace_states.clone());
        app.update_keybinds(&keybinds, cfg);
    }

    // Connect to OpenRGB
    let client = OpenRgbClient::connect().await?;

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

    // The modes to recognise: the config's, plus those keybinds light keys in
    fn build_modes(
        config: &Option<orkh::config::Config>,
        applicator: Option<&LedApplicator>,
    ) -> HashSet<String> {
        match config {
            Some(cfg) => {
                let mut modes = cfg.modes.keys().cloned().collect::<HashSet<_>>();
                modes.extend(
                    applicator
                        .into_iter()
                        .flat_map(|a| a.keybind_modes().cloned()),
                );
                modes
            }
            None => {
                // Default modes if no config
                [
                    "super",
                    "shift",
                    "ctrl",
                    "alt",
                    "super_shift",
                    "super_ctrl",
                    "super_alt",
                    "shift_ctrl",
                    "shift_alt",
                    "ctrl_alt",
                ]
                .into_iter()
                .map(String::from)
                .collect()
            }
        }
    }

    let mut modes = build_modes(&config, applicator.as_ref());
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
                        app.update_keybinds(&keybinds, cfg);
                    }
                    // Rebuild modes from new config
                    modes = build_modes(&config, applicator.as_ref());
                }

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
            Ok(states) = workspace_rx.recv() => {
                workspace_states = states;

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

            // duckwm's keybinds changed (a reload, or we just connected)
            Ok(binds) = keybind_rx.recv() => {
                keybinds = binds;
                if let (Some(cfg), Some(app)) = (config.as_ref(), applicator.as_mut()) {
                    app.update_keybinds(&keybinds, cfg);
                }
                modes = build_modes(&config, applicator.as_ref());

                let curr_mode = keyboard.state.get_current_mode(&modes).await;

                if let (Some(cfg), Some(app)) = (config.as_ref(), applicator.as_ref()) {
                    let mut cmd = controller.cmd();
                    if let Err(e) = app.apply_mode(&curr_mode, cfg, &mut cmd, &zone) {
                        eprintln!("Error applying mode after keybind update: {}", e);
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
