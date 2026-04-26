use evdev::{Device, EventType, KeyCode};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

/// Shared state for pressed keys (ordered)
#[derive(Clone)]
pub struct KeyboardState {
    pressed: Arc<Mutex<HashSet<String>>>,
    order: Arc<Mutex<VecDeque<String>>>,
}

impl Default for KeyboardState {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardState {
    pub fn new() -> Self {
        Self {
            pressed: Arc::new(Mutex::new(HashSet::new())),
            order: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub async fn is_pressed(&self, key: &str) -> bool {
        self.pressed.lock().await.contains(key)
    }

    pub async fn current_keys(&self) -> Vec<String> {
        let pressed = self.pressed.lock().await;
        let order = self.order.lock().await;
        
        // Only return keys that are currently pressed, in order
        order.iter()
            .filter(|k| pressed.contains(*k))
            .cloned()
            .collect()
    }

    /// Determine current mode from all pressed keys
    pub async fn get_current_mode(&self, modes: &HashSet<String>) -> String {
        let pressed = self.pressed.lock().await;
        
        if pressed.is_empty() {
            return "base".to_string();
        }

        let modifiers = ["super", "shift", "alt", "ctrl"];
        
        let order = self.order.lock().await;
        
        // Get keys in order that are still pressed
        let current_pressed_in_order: Vec<String> = order
            .iter()
            .filter(|k| pressed.contains(*k))
            .cloned()
            .collect();
        
        if current_pressed_in_order.is_empty() {
            return "base".to_string();
        }
        
        // Try longest sequences first (N → 1)
        let len = current_pressed_in_order.len();
        let seq = current_pressed_in_order
            .iter()
            .skip(current_pressed_in_order.len() - len)
            .cloned()
            .collect::<Vec<_>>()
            .join("_");

        if modes.contains(&seq) {
            return seq;
        }

        let pressed_modifiers: Vec<&String> = pressed
            .iter()
            .filter(|k| pressed.contains(*k) && modifiers.contains(&k.as_str()))
            .collect();

        if !pressed_modifiers.is_empty() {
            let modifier_sequence = pressed_modifiers
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("_");
            if modes.contains(&modifier_sequence) {
                return modifier_sequence;
            }
        }

        "base".to_string()
    }

    async fn press(&self, key: &str) {
        let mut pressed = self.pressed.lock().await;
        let mut order = self.order.lock().await;

        if pressed.insert(key.to_string()) {
            order.push_back(key.to_string());
        }
    }

    async fn release(&self, key: &str) {
        let mut pressed = self.pressed.lock().await;
        let mut order = self.order.lock().await;

        if pressed.remove(key) {
            // Remove ALL occurrences of this key from order
            order.retain(|k| k != key);
        }
        
        // If no keys are pressed, clear the entire order
        // This handles the "release all" case
        if pressed.is_empty() {
            order.clear();
        }
    }
}

/// Public listener handle
pub struct KeyboardListener {
    pub state: KeyboardState,
}

impl KeyboardListener {
    pub fn start() -> Self {
        let state = KeyboardState::new();
        tokio::spawn(start_listening(state.clone()));
        Self { state }
    }
}

fn normalize_key_code(key: KeyCode) -> String {
    match key {
        KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => "super".into(),
        KeyCode::KEY_LEFTALT | KeyCode::KEY_RIGHTALT => "alt".into(),
        KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT => "shift".into(),
        KeyCode::KEY_LEFTCTRL | KeyCode::KEY_RIGHTCTRL => "ctrl".into(),
        _ => format!("{:?}", key)
            .strip_prefix("KEY_")
            .unwrap_or("unknown")
            .to_lowercase(),
    }
}

fn find_all_physical_keyboards() -> Vec<Device> {
    let mut keyboards = Vec::new();

    for entry in fs::read_dir("/dev/input").unwrap() {
        let Ok(entry) = entry else { continue };
        let path = entry.path();

        if !path.to_string_lossy().starts_with("/dev/input/event") {
            continue;
        }

        if let Ok(device) = Device::open(&path) {
            if let Some(keys) = device.supported_keys() {
                if keys.contains(KeyCode::KEY_A) {
                    device.set_nonblocking(true).ok();
                    keyboards.push(device);
                }
            }
        }
    }

    keyboards
}

async fn start_listening(state: KeyboardState) {
    let devices = find_all_physical_keyboards();

    for mut device in devices {
        let state_clone = state.clone();
        tokio::spawn(async move {
            // Use a simple loop with fetch_events
            loop {
                match device.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            if ev.event_type() == EventType::KEY {
                                let key = normalize_key_code(KeyCode::new(ev.code()));
                                match ev.value() {
                                    1 => state_clone.press(&key).await,
                                    0 => state_clone.release(&key).await,
                                    _ => {}
                                }
                            }
                        }
                    }
                    Err(e) => {
                        // Check if it's a WouldBlock error
                        if let Some(raw_os_error) = e.raw_os_error() {
                            if raw_os_error == libc::EAGAIN || raw_os_error == libc::EWOULDBLOCK {
                                // No events available, sleep to reduce CPU usage
                                sleep(Duration::from_millis(10)).await;
                                continue;
                            }
                        }
                        eprintln!("Error reading events: {}", e);
                        break;
                    }
                }
            }
        });
    }
}
