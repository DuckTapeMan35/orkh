use std::collections::{BTreeMap, HashMap};

use duckwm::{KeyCombo, Mods};

use crate::config::{ColorSpec, Rule};

/// The modifiers orkh tracks, as mode names spell them.
const MODIFIERS: [(Mods, &str); 4] = [
    (Mods::SUPER, "super"),
    (Mods::CTRL, "ctrl"),
    (Mods::ALT, "alt"),
    (Mods::SHIFT, "shift"),
];

/// The modes a combination lights its key in: every order its modifiers can
/// be pressed in (super_shift and shift_super), or "base" without any. None
/// if it needs a modifier orkh doesn't track (caps lock, num lock, ...).
pub fn modes_for(mods: Mods) -> Option<Vec<String>> {
    let tracked = MODIFIERS.iter().fold(Mods::NONE, |all, (m, _)| all | *m);
    if mods.0 & !tracked.0 != 0 {
        return None;
    }
    let names: Vec<&str> = MODIFIERS
        .iter()
        .filter(|(m, _)| mods.contains(*m))
        .map(|(_, n)| *n)
        .collect();
    if names.is_empty() {
        return Some(vec!["base".to_string()]);
    }
    let mut out = Vec::new();
    orders(&names, &mut Vec::new(), &mut out);
    Some(out)
}

/// Every order of names, joined with "_".
fn orders(rest: &[&str], so_far: &mut Vec<String>, out: &mut Vec<String>) {
    if rest.is_empty() {
        out.push(so_far.join("_"));
        return;
    }
    for i in 0..rest.len() {
        let mut others = rest.to_vec();
        so_far.push(others.remove(i).to_string());
        orders(&others, so_far, out);
        so_far.pop();
    }
}

/// duckwm's keysym name as OpenRGB names the key, the way orkh matches LEDs:
/// "Return" is "enter", "comma" is ",". None for keys with no LED of their own.
pub fn key_name(keysym: &str) -> Option<String> {
    let mut chars = keysym.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_lowercase().to_string());
        }
    }
    if let Some(n) = keysym.strip_prefix('F').filter(|n| n.parse::<u8>().is_ok()) {
        return Some(format!("f{n}"));
    }
    let name = match keysym {
        "Return" => "enter",
        "Escape" => "escape",
        "Tab" => "tab",
        "BackSpace" => "backspace",
        "space" => "space",
        "Up" => "up arrow",
        "Down" => "down arrow",
        "Left" => "left arrow",
        "Right" => "right arrow",
        "Print" => "print screen",
        "Scroll_Lock" => "scroll lock",
        "Pause" => "pause/break",
        "Insert" => "insert",
        "Delete" => "delete",
        "Home" => "home",
        "End" => "end",
        "Prior" | "Page_Up" => "page up",
        "Next" | "Page_Down" => "page down",
        "Menu" => "menu",
        "Caps_Lock" => "caps lock",
        "grave" => "`",
        "minus" => "-",
        "equal" => "=",
        "bracketleft" => "[",
        "bracketright" => "]",
        "backslash" => "\\ (ansi)",
        "semicolon" => ";",
        "apostrophe" => "'",
        "comma" => ",",
        "period" => ".",
        "slash" => "/",
        _ => return None,
    };
    Some(name.to_string())
}

/// For each mode, a rule per bound key. Within each set of modifiers the keys
/// are in a fixed (sorted) order and take the colors in turn, so a key keeps
/// its color from one run to the next while the binds stay the same.
pub fn rules(combos: &[KeyCombo], colors: &[ColorSpec]) -> HashMap<String, Vec<Rule>> {
    let mut out: HashMap<String, Vec<Rule>> = HashMap::new();
    if colors.is_empty() {
        return out;
    }
    let mut by_mods: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for c in combos {
        if let Some(key) = key_name(&c.key) {
            by_mods.entry(c.mods.0).or_default().push(key);
        }
    }
    for (mods, mut keys) in by_mods {
        let Some(modes) = modes_for(Mods(mods)) else {
            continue;
        };
        keys.sort();
        keys.dedup();
        let lit: Vec<Rule> = keys
            .into_iter()
            .zip(colors.iter().cycle())
            .map(|(key, color)| Rule {
                keys: vec![key],
                color: color.clone(),
                condition: None,
                value: None,
            })
            .collect();
        for mode in modes {
            out.entry(mode).or_default().extend(lit.iter().cloned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(s: &str) -> KeyCombo {
        s.parse().unwrap()
    }

    #[test]
    fn modes_in_every_press_order() {
        let mut m = modes_for(Mods::SUPER | Mods::SHIFT).unwrap();
        m.sort();
        assert_eq!(m, ["shift_super", "super_shift"]);
        assert_eq!(modes_for(Mods::NONE).unwrap(), ["base"]);
        assert_eq!(
            modes_for(Mods::SUPER | Mods::CTRL | Mods::ALT)
                .unwrap()
                .len(),
            6
        );
        assert_eq!(modes_for(Mods::SUPER | Mods::MOD2), None);
    }

    #[test]
    fn keysyms_to_led_names() {
        assert_eq!(key_name("q").as_deref(), Some("q"));
        assert_eq!(key_name("Q").as_deref(), Some("q"));
        assert_eq!(key_name("5").as_deref(), Some("5"));
        assert_eq!(key_name("Return").as_deref(), Some("enter"));
        assert_eq!(key_name("F11").as_deref(), Some("f11"));
        assert_eq!(key_name("comma").as_deref(), Some(","));
        assert_eq!(key_name("XF86AudioMute"), None);
    }

    #[test]
    fn colors_cycle_per_modifier_set() {
        let red = ColorSpec::Rgb([255, 0, 0]);
        let blue = ColorSpec::Rgb([0, 0, 255]);
        let r = rules(
            &[
                combo("super+b"),
                combo("super+a"),
                combo("super+c"),
                combo("super+shift+q"),
            ],
            &[red.clone(), blue.clone()],
        );
        let lit = |mode: &str| -> Vec<(String, [u8; 3])> {
            r[mode]
                .iter()
                .map(|rule| {
                    let ColorSpec::Rgb(c) = rule.color;
                    (rule.keys[0].clone(), c)
                })
                .collect()
        };
        assert_eq!(
            lit("super"),
            [
                ("a".into(), [255, 0, 0]),
                ("b".into(), [0, 0, 255]),
                ("c".into(), [255, 0, 0])
            ]
        );
        assert_eq!(lit("super_shift"), [("q".into(), [255, 0, 0])]);
        assert_eq!(lit("shift_super"), lit("super_shift"));
        assert!(rules(&[combo("super+a")], &[]).is_empty());
    }
}
