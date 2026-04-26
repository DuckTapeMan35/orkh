use openrgb2::{Color, Zone, Command};
use crate::config::{Config, ColorSpec, KeyPosition, Condition, Value, Rule};
use std::collections::{HashMap, HashSet};

pub struct LedApplicator {
    key_mapping: HashMap<String, Vec<String>>,
    workspace_states: Vec<Value>,
    last_non_empty_workspaces: HashSet<String>,
    wm_integration_enabled: bool,
}

impl LedApplicator {
    pub fn new(config: &Config) -> Self {
        
        // Build key mapping from config
        let mut key_mapping = HashMap::new();
        
        for (key_name, position) in &config.key_positions {
            match position {
                KeyPosition::Single(key) => {
                    let normalized = normalize_key_name(key);
                    key_mapping.insert(key_name.clone(), vec![normalized]);
                }
                KeyPosition::Multiple(keys) => {
                    let normalized: Vec<String> = keys.iter()
                        .map(|k| normalize_key_name(k))
                        .collect();
                    key_mapping.insert(key_name.clone(), normalized);
                }
            }
        }
        
        Self {
            key_mapping,
            workspace_states: Vec::new(),
            last_non_empty_workspaces: HashSet::new(),
            wm_integration_enabled: true,
        }
    }
    
    /// Update workspace states
    pub fn update_workspace_states(&mut self, states: Vec<Value>) {
        self.workspace_states = states;
        
        // Update last_non_empty_workspaces based on workspace states
        self.last_non_empty_workspaces.clear();
        for (i, state) in self.workspace_states.iter().enumerate() {
            match state {
                Value::Active | Value::Focused => {
                    self.last_non_empty_workspaces.insert((i + 1).to_string());
                }
                Value::Inactive => {} // Don't add inactive workspaces
            }
        }
    }
    
    /// Enable/disable window manager integration
    pub fn set_wm_integration(&mut self, enabled: bool) {
        self.wm_integration_enabled = enabled;
    }
    
    pub fn apply_mode(
        &self,
        mode_name: &str,
        config: &Config,
        cmd: &mut Command,
        zone: &Zone,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create a desired_state map: LED ID -> Color
        let mut desired_state: HashMap<usize, Color> = HashMap::new();
        
        // Get the mode rules
        if let Some(mode) = config.modes.get(mode_name) {
            // Apply rules in the order they appear in the config
            for rule in &mode.rules {
                self.apply_rule(rule, &mut desired_state, zone)?;
            }
        }
        
        // Apply the accumulated desired state to the LEDs
        self.apply_desired_state(&desired_state, cmd, zone)
    }
    
    /// Apply a lighting rule to the desired state
    fn apply_rule(
        &self,
        rule: &Rule,
        desired_state: &mut HashMap<usize, Color>,
        zone: &Zone,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Get key positions (LED IDs)
        let keys = &rule.keys;
        let positions = self.get_keys_positions(keys, zone);
        
        if positions.is_empty() {
            return Ok(());
        }
        
        // Handle conditional rules
        if let Some(condition) = &rule.condition {
            match condition {
                Condition::WorkSpaces => {
                    // Skip if workspace integration not enabled
                    if !self.wm_integration_enabled {
                        return Ok(());
                    }
                    
                    if let Some(value) = &rule.value {
                        let color = self.resolve_color(&rule.color);
                        
                        // Only apply to number keys
                        if keys.len() == 1 && keys[0] == "numbers" {
                            // Get the actual number keys from the mapping
                            if let Some(number_keys) = self.key_mapping.get("numbers") {
                                // Map each number key to a workspace (1-9, skip 0 since no workspace 10)
                                for (i, key_name) in number_keys.iter().enumerate() {
                                    if let Some(led_id) = self.find_led_for_key(key_name, zone) {
                                        let workspace_index = i;
                                        
                                        let should_apply = match value {
                                            Value::Focused => {
                                                // Check if workspace is focused
                                                if let Some(state) = self.workspace_states.get(workspace_index) {
                                                    matches!(state, Value::Focused)
                                                } else {
                                                    false
                                                }
                                            }
                                            Value::Active => {
                                                // Check if workspace is active (not focused)
                                                if let Some(state) = self.workspace_states.get(workspace_index) {
                                                    matches!(state, Value::Active)
                                                } else {
                                                    false
                                                }
                                            }
                                            Value::Inactive => {
                                                // Check if workspace is inactive
                                                if let Some(state) = self.workspace_states.get(workspace_index) {
                                                    matches!(state, Value::Inactive)
                                                } else {
                                                    false
                                                }
                                            }
                                        };
                                        
                                        if should_apply {
                                            desired_state.insert(led_id, color);
                                        }
                                    }
                                }
                            }
                        }
                        return Ok(());
                    }
                }
            }
        }
        
        // Handle simple color rule
        let color = self.resolve_color(&rule.color);

        for &led_id in &positions {
            desired_state.insert(led_id, color);
        }
        
        Ok(())
    }
    
    /// Get LED IDs for a list of key names
    fn get_keys_positions(&self, keys: &[String], zone: &Zone) -> Vec<usize> {
        let mut positions = Vec::new();
        
        for key_spec in keys {
            if key_spec == "all" {
                // Add all LED IDs
                positions.extend(zone.led_iter().map(|led| led.id()));
                continue;
            }
            
            if let Some(key_names) = self.key_mapping.get(key_spec) {
                for key_name in key_names {
                    if let Some(led_id) = self.find_led_for_key(key_name, zone) {
                        positions.push(led_id);
                    }
                }
            } else {
                // Try direct key name lookup
                if let Some(led_id) = self.find_led_for_key(key_spec, zone) {
                    positions.push(led_id);
                }
            }
        }
        
        // Remove duplicates
        positions.sort();
        positions.dedup();
        positions
    }
    
    /// Find LED ID for a specific key name
    fn find_led_for_key(&self, key_name: &str, zone: &Zone) -> Option<usize> {
        let normalized_key_name = normalize_key_name(key_name);
        
        for led in zone.led_iter() {
            let led_name = normalize_led_name(led.name());
            if led_name == normalized_key_name {
                return Some(led.id());
            }
        }
        
        None
    }
    
    /// Apply the desired state to the LEDs via command
    fn apply_desired_state(
        &self,
        desired_state: &HashMap<usize, Color>,
        cmd: &mut Command,
        zone: &Zone,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // First, turn all LEDs off
        for led in zone.led_iter() {
            cmd.set_led(led.id(), Color::new(0, 0, 0))?;
        }
        
        // Then apply the desired colors
        for (&led_id, &color) in desired_state {
            cmd.set_led(led_id, color)?;
        }
        
        Ok(())
    }
    
    fn resolve_color(&self, color_spec: &ColorSpec) -> Color {
        match color_spec {
            ColorSpec::Rgb([r, g, b]) => {
                Color::new(*r, *g, *b)
            }
        }
    }
    
    /// Check if a specific workspace matches a value
    pub fn workspace_matches(&self, workspace_index: usize, value: &Value) -> bool {
        if let Some(state) = self.workspace_states.get(workspace_index) {
            state == value
        } else {
            false
        }
    }
    
    /// Get LED IDs for number keys from the "numbers" group
    pub fn get_number_key_leds(&self, zone: &Zone) -> Vec<usize> {
        let mut leds = Vec::new();
        if let Some(number_keys) = self.key_mapping.get("numbers") {
            for key_name in number_keys {
                if let Some(led_id) = self.find_led_for_key(key_name, zone) {
                    leds.push(led_id);
                }
            }
        }
        leds
    }
}

fn normalize_key_name(key: &str) -> String {
    key.trim()
        .to_lowercase()
        .replace("_", " ")
        .replace("-", " ")
}

fn normalize_led_name(led_name: &str) -> String {
    led_name
        .replace("LED:", "")
        .replace("Key:", "")
        .trim()
        .to_lowercase()
        .replace("_", " ")
        .replace("-", " ")
}
