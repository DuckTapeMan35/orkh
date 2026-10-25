use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use duckwm::{Client, KeyCombo, WorkspaceState};
use tokio::sync::broadcast;
use tokio::time::{interval, sleep, Duration, MissedTickBehavior};

use crate::config::Value;

/// Number keys 1-9 and 0: workspace positions 1-10.
const SLOTS: usize = 10;

/// Keybinds change only on a config reload: asked for every this many polls (2s).
const KEYBINDS_EVERY: u32 = 20;

fn to_values(states: &[duckwm::Workspace]) -> Vec<Value> {
    let mut values = vec![Value::Inactive; SLOTS];
    for ws in states {
        let i = ws.position as usize;
        if (1..=SLOTS).contains(&i) {
            values[i - 1] = match ws.state {
                WorkspaceState::Focused => Value::Focused,
                WorkspaceState::Active => Value::Active,
                WorkspaceState::Inactive => Value::Inactive,
            };
        }
    }
    values
}

/// Store v as the latest and announce it, unless it's what was there.
fn publish<T: Clone + PartialEq>(v: T, latest: &Mutex<Option<T>>, tx: &broadcast::Sender<T>) {
    let mut latest = latest.lock().unwrap();
    if latest.as_ref() != Some(&v) {
        *latest = Some(v.clone());
        let _ = tx.send(v);
    }
}

/// Poll duckwm's workspace states (and, less often, its keybinds) over one
/// connection, publishing them when they change; reconnect whenever duckwm
/// goes away.
pub async fn monitor(
    socket: PathBuf,
    workspace_tx: broadcast::Sender<Vec<Value>>,
    latest_workspace: Arc<Mutex<Option<Vec<Value>>>>,
    keybind_tx: broadcast::Sender<Vec<KeyCombo>>,
    latest_keybinds: Arc<Mutex<Option<Vec<KeyCombo>>>>,
) {
    loop {
        let wm = match Client::connect_to(&socket).await {
            Ok(wm) => wm,
            Err(_) => {
                sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let mut tick = interval(Duration::from_millis(100));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        for n in 0u32.. {
            tick.tick().await;
            let Ok(states) = wm.get_workspace_states().await else {
                break; // duckwm went away: connect again
            };
            publish(to_values(&states), &latest_workspace, &workspace_tx);
            if n % KEYBINDS_EVERY == 0 {
                let Ok(mut binds) = wm.get_keybinds().await else {
                    break;
                };
                binds.sort_by_key(|c| (c.mods.0, c.key.clone())); // duckwm's order is a hash map's
                publish(binds, &latest_keybinds, &keybind_tx);
            }
        }
    }
}
