use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use duckwm::{Client, WorkspaceState};
use tokio::sync::broadcast;
use tokio::time::{interval, sleep, Duration, MissedTickBehavior};

use crate::config::Value;

/// Number keys 1-9 and 0: workspace positions 1-10.
const SLOTS: usize = 10;

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

/// Poll duckwm's workspace states over one connection, publishing them when
/// they change; reconnect whenever duckwm goes away.
pub async fn monitor_workspaces(
    socket: PathBuf,
    workspace_tx: broadcast::Sender<Vec<Value>>,
    latest_workspace: Arc<Mutex<Option<Vec<Value>>>>,
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
        loop {
            tick.tick().await;
            let Ok(states) = wm.get_workspace_states().await else {
                break; // duckwm went away: connect again
            };
            let values = to_values(&states);
            let mut latest = latest_workspace.lock().unwrap();
            if latest.as_ref() != Some(&values) {
                *latest = Some(values.clone());
                let _ = workspace_tx.send(values);
            }
        }
    }
}
