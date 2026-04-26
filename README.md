# orkh — OpenRGB keyboard highlighter

`orkh` highlights keyboard keys while they are held, based on a YAML configuration file. It is written in **Rust** and uses **OpenRGB** for LED control.

It also supports **workspace-aware highlighting** (currently only mangowm, other compositors/wms to follow).

## Requirements

- **OpenRGB** (the `openrgb` binary must be available)
- A keyboard supported by OpenRGB with individually lit keys
- Linux input event access
- Rust toolchain (only if building from source): `cargo`, `rustc`

## Install / Setup (systemd)

This repository includes a setup script that:
- builds `orkh` in release mode
- installs it to `/usr/bin/orkh`
- creates a systemd service at `/etc/systemd/system/orkh.service` (more init systems will be supported in the future, but you could make an equivalent service manually)
- creates the user config directory at `~/.config/orkh/`

```bash
git clone https://github.com/DuckTapeMan35/orkh
cd orkh
chmod +x setup.sh
./setup.sh
```

After installation you can control the service with:

```bash
sudo systemctl status orkh.service
sudo systemctl restart orkh.service
```

Notes:
- `orkh` starts an OpenRGB server process (`openrgb --server`) and waits ~5 seconds to let it initialize.
- The service sets `ORKH_USER` so the root-running service can still read the config from your user’s home directory.
- As a result of needing to read input events, making ipc calls and reading the config the safest option is to run this as a root service, with basically no way to create a custom user for the service. Until wayland adds a way to read arbitrary input (much like the x11 api which has widespread misunderstandings over its security) this is the only option to make this work on it.

## Configuration

Config file location:

- `~/.config/orkh/config.yaml`

### Example config

```yaml
window_manager: mangowc
log_level: critical

key_positions:
  # Define positions for individual keys. Values can be key names.
  q:
    - q
  x:
    - x
  i:
    - i
  s:
    - s
  e:
    - e
  n:
    - n
  g:
    - g
  r:
    - r
  c:
    - c
  a:
    - a
  f:
    - f
  print_screen:
    - print screen
    - p

  # Numbers must be inside quotes or they will be cast into integers
  1_key: ['1']
  2_key: ['2']
  3_key: ['3']
  4_key: ['4']
  5_key: ['5']
  6_key: ['6']
  7_key: ['7']
  8_key: ['8']
  9_key: ['9']
  0_key: ['0']

  # Special keys
  super: [left windows]
  enter: [enter]
  shift: [left shift, right shift]
  tab: [tab]
  space: [space]
  alt: [left alt, right alt]

  # Groups
  numbers: ['1','2','3','4','5','6','7','8','9','0']
  layouts: [t, s, m, g, d, c, v]
  arrows: [right arrow, left arrow, up arrow, down arrow]

modes:
  # Base mode - applied when no keys are pressed
  base:
    rules:
      - keys: [all]
        color: "#4ebc42"
      - keys: [numbers]
        condition: workspaces
        value: active
        color: "#c1c5c3"
      - keys: [numbers]
        condition: workspaces
        value: inactive
        color: "#31465f"
      - keys: [numbers]
        condition: workspaces
        value: focused
        color: "#4ebc42"

  # Single-key mode
  super:
    rules:
      - keys: [numbers]
        condition: workspaces
        value: active
        color: "#c1c5c3"
      - keys: [numbers]
        condition: workspaces
        value: inactive
        color: "#31465f"
      - keys: [numbers]
        condition: workspaces
        value: focused
        color: "#4ebc42"
      - keys: [super]
        color: [255, 255, 255]
      - keys: [enter]
        color: "#c1c5c3"
      - keys: [d]
        color: "#4ebc42"
      - keys: [x]
        color: [255, 0, 0]
      - keys: [i]
        color: "#439340"
      - keys: [arrows]
        color: [255,255,255]
      - keys: [shift]
        color: [255, 255, 255]
      - keys: [tab]
        color: [255,255,255]
      - keys: [e]
        color: "#425e7f"
      - keys: [n]
        color: "#73a146"
      - keys: [print_screen]
        color: "#99e34e"
      - keys: [g]
        color: "#55799d"
      - keys: [r]
        color: "#4ebc42"
      - keys: [space]
        color: "#439340"
      - keys: [c]
        color: "#425e7f"
      - keys: [a]
        color: "#73a146"

  # Ordered combo: super then shift
  super_shift:
    rules:
      - keys: [numbers]
        condition: workspaces
        value: active
        color: "#c1c5c3"
      - keys: [numbers]
        condition: workspaces
        value: inactive
        color: "#31465f"
      - keys: [numbers]
        condition: workspaces
        value: focused
        color: "#4ebc42"
      - keys: [super]
        color: [255, 255, 255]
      - keys: [q]
        color: [255, 0, 0]
      - keys: [e]
        color: [255, 0, 0]
      - keys: [shift]
        color: [255, 255, 255]
      - keys: [arrows]
        color: [255,255,255]
      - keys: [layouts]
        color: "#4ebc42"
      - keys: [n]
        color: "#439340"

  # Reverse order: shift then super
  shift_super:
    rules:
      - keys: [numbers]
        condition: workspaces
        value: active
        color: "#c1c5c3"
      - keys: [numbers]
        condition: workspaces
        value: inactive
        color: "#31465f"
      - keys: [numbers]
        condition: workspaces
        value: focused
        color: "#4ebc42"
      - keys: [super]
        color: [255, 255, 255]
      - keys: [q]
        color: [255, 0, 0]
      - keys: [e]
        color: [255, 0, 0]
      - keys: [shift]
        color: [255, 255, 255]
      - keys: [arrows]
        color: [255,255,255]
      - keys: [layouts]
        color: "#4ebc42"
      - keys: [n]
        color: "#439340"

  alt:
    rules:
      - keys: [numbers]
        condition: workspaces
        value: active
        color: "#c1c5c3"
      - keys: [numbers]
        condition: workspaces
        value: inactive
        color: "#31465f"
      - keys: [numbers]
        condition: workspaces
        value: focused
        color: "#4ebc42"
      - keys: [alt]
        color: [255,255,255]
      - keys: [shift]
        color: [255,255,255]
      - keys: [tab]
        color: "#4ebc42"
      - keys: [f]
        color: "#439340"
      - keys: [i]
        color: "#425e7f"
      - keys: [c]
        color: "#73a146"

  alt_shift:
    rules:
      - keys: [numbers]
        condition: workspaces
        value: active
        color: "#c1c5c3"
      - keys: [numbers]
        condition: workspaces
        value: inactive
        color: "#31465f"
      - keys: [numbers]
        condition: workspaces
        value: focused
        color: "#4ebc42"
      - keys: [i]
        color: "#4ebc42"
      - keys: [alt, shift]
        color: [255,255,255]

  shift_alt:
    rules:
      - keys: [numbers]
        condition: workspaces
        value: active
        color: "#c1c5c3"
      - keys: [numbers]
        condition: workspaces
        value: inactive
        color: "#31465f"
      - keys: [numbers]
        condition: workspaces
        value: focused
        color: "#4ebc42"
      - keys: [i]
        color: "#4ebc42"
      - keys: [alt, shift]
        color: [255,255,255]
```

## How it works

- `orkh` reads keypresses from Linux input events and determines the current “mode” based on which keys are held.
- A mode is selected by name:
  - single key: `super`
  - ordered combos: `super_shift`, `shift_super`, etc.
- Rules within a mode are applied in order; later rules can override earlier ones.

## Workspace integration (mangowc)

Set:

```yaml
window_manager: mangowc # or mango or mangowm
```

Then you can use:

```yaml
condition: workspaces
value: active|inactive|focused
```

to color keys (commonly number keys) based on workspace state.

## Color formats

`color` can be either:
- a hex string: `"#4ebc42"`
- an RGB array: `[255, 255, 255]`
