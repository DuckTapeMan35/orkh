use crate::config::{Config, KeyPosition, Mode, Rule, ColorSpec, Condition, Value};
use yaml_rust2::Yaml;
use std::collections::HashMap;

pub fn parse_config(yaml: &yaml_rust2::Yaml) -> Option<Config> {
    Some(Config {
        window_manager: yaml["window_manager"].as_str().unwrap_or("unknown").to_string(),
        log_level: yaml["log_level"].as_str().unwrap_or("info").to_string(),
        key_positions: parse_key_positions(&yaml["key_positions"]),
        modes: parse_modes(&yaml["modes"]),
    })
}

fn parse_key_positions(yaml: &Yaml) -> HashMap<String, KeyPosition> {
    let mut result = HashMap::new();
    
    if let Yaml::Hash(hash) = yaml {
        for (key, value) in hash {
            let key_name = key.as_str().unwrap_or("").to_string();
            
            if let Yaml::Array(arr) = value {
                let strings: Vec<String> = arr.iter()
                    .filter_map(|y| y.as_str())
                    .map(|s| s.to_string())
                    .collect();
                result.insert(key_name, KeyPosition::Multiple(strings));
            } else if let Some(s) = value.as_str() {
                result.insert(key_name, KeyPosition::Single(s.to_string()));
            } else if let Yaml::Integer(i) = value {
                result.insert(key_name, KeyPosition::Single(i.to_string()));
            }
        }
    }
    
    result
}

fn parse_modes(yaml: &Yaml) -> HashMap<String, Mode> {
    let mut result = HashMap::new();

    if let Yaml::Hash(hash) = yaml {
        for (mode_name, mode_hash) in hash.iter().filter_map(|(k, v)| {
            if let Yaml::Hash(h) = v {
                Some((k, h))
            } else {
                None
            }
        }) {
            let mode_name_str = mode_name.as_str().unwrap_or("").to_string();

            if let Some(Yaml::Array(rules_array)) =
                mode_hash.get(&Yaml::String("rules".to_string()))
            {
                let rules = rules_array
                    .iter()
                    .filter_map(parse_rule)
                    .collect();

                result.insert(mode_name_str, Mode { rules });
            }
        }
    }
    result
}


fn parse_rule(yaml: &Yaml) -> Option<Rule> {
    if let Yaml::Hash(rule_hash) = yaml {
        let keys_yaml = rule_hash.get(&Yaml::String("keys".to_string()))?;
        let color_yaml = rule_hash.get(&Yaml::String("color".to_string()))?;
        
        let keys = parse_keys(keys_yaml);
        let color = parse_color_spec(color_yaml)?;
        
        // Parse optional condition and value
        let condition = rule_hash.get(&Yaml::String("condition".to_string()))
            .and_then(parse_condition);
        let value = rule_hash.get(&Yaml::String("value".to_string()))
            .and_then(parse_value);
        
        Some(Rule { keys, color, condition, value })
    } else {
        None
    }
}

fn parse_keys(yaml: &Yaml) -> Vec<String> {
    match yaml {
        Yaml::Array(arr) => arr.iter()
            .filter_map(|y| {
                if let Yaml::String(s) = y {
                    Some(s.clone())
                } else if let Yaml::Integer(i) = y {
                    Some(i.to_string())
                } else {
                    None
                }
            })
            .collect(),
        Yaml::String(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn parse_color_spec(yaml: &Yaml) -> Option<ColorSpec> {
    match yaml {
        Yaml::String(s) => {
            if s.starts_with('#') && s.len() == 7 {
                let r = u8::from_str_radix(&s[1..3], 16).ok()?;
                let g = u8::from_str_radix(&s[3..5], 16).ok()?;
                let b = u8::from_str_radix(&s[5..7], 16).ok()?;
                Some(ColorSpec::Rgb([r, g, b]))
            } else {
                None
            }
        },
        Yaml::Array(arr) => {
            if arr.len() == 3 {
                let r = arr[0].as_i64()? as u8;
                let g = arr[1].as_i64()? as u8;
                let b = arr[2].as_i64()? as u8;
                Some(ColorSpec::Rgb([r, g, b]))
            } else {
                None
            }
        },
        _ => None,
    }
}

fn parse_condition(yaml: &Yaml) -> Option<Condition> {
    match yaml {
        Yaml::String(s) => match s.as_str() {
            "workspaces" => Some(Condition::WorkSpaces),
            _ => None,
        },
        _ => None,
    }
}

fn parse_value(yaml: &Yaml) -> Option<Value> {
    match yaml {
        Yaml::String(s) => match s.as_str() {
            "active" => Some(Value::Active),
            "inactive" => Some(Value::Inactive),
            "focused" => Some(Value::Focused),
            _ => None,
        },
        _ => None,
    }
}

pub fn parse_mmsg_output(lines: &[&str]) -> Vec<Value> {
    let mut values = Vec::new();

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();

        // Match: HDMI-A-1 tag <num> <selected> <windows> <focused>
        if parts.len() >= 6 && parts[1] == "tag" {
            let tag_num = parts[2];
            let windows = parts[4].parse::<u32>();
            let focused = parts[3].parse::<u32>();

            if let (Ok(windows), Ok(focused)) = (windows, focused) {
                // Only tags 1–9, same as Python
                if let Ok(tag) = tag_num.parse::<u32>() {
                    if (1..=9).contains(&tag) {
                        let value = if focused == 1 {
                            Value::Focused  // Window has focus (highest priority)
                        } else if windows > 0 {
                            Value::Active   // Has windows but not visible
                        } else {
                            Value::Inactive // No windows and not visible
                        };

                        values.push(value);
                    }
                }
            }
        }
    }

    values
}
