use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

/// Set by the flake (devShell and package) to a pinned store path.
/// Falls back to the usual FHS location elsewhere.
const BWRAP: &str = match option_env!("ORKH_BWRAP") {
    Some(p) => p,
    None => "/usr/bin/bwrap",
};

/// Marker telling the re-exec'd instance it is already inside the sandbox.
const SANDBOXED: &str = "ORKH_SANDBOXED";

/// Where OpenRGB keeps its config/profiles (mounted at /root/.config/OpenRGB).
const OPENRGB_STATE: &str = "/var/lib/orkh/openrgb";

fn push(v: &mut Vec<String>, parts: &[&str]) {
    v.extend(parts.iter().map(|s| s.to_string()));
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Whether to expose /dev/i2c-* (RGB RAM, motherboard, GPU lighting).
/// Read before the sandbox exists, so toggling it requires a restart.
fn i2c_enabled(cfg_file: &str) -> bool {
    if std::env::var_os("ORKH_I2C").is_some() {
        return true; // override for testing
    }
    fs::read_to_string(cfg_file)
        .ok()
        .and_then(|s| yaml_rust2::YamlLoader::load_from_str(&s).ok())
        .and_then(|docs| docs.into_iter().next())
        .and_then(|y| y["openrgb"]["i2c"].as_bool())
        .unwrap_or(false)
}

/// Re-execs the current binary inside bubblewrap. Returns only when already
/// sandboxed (or when explicitly disabled). Fails closed: if bwrap can't be
/// exec'd, the process exits instead of running unsandboxed.
pub fn ensure_sandboxed() {
    if std::env::var_os(SANDBOXED).is_some() {
        return;
    }
    if std::env::var_os("ORKH_NO_SANDBOX").is_some() {
        eprintln!("orkh: WARNING: running without sandbox (ORKH_NO_SANDBOX set)");
        return;
    }

    let user = std::env::var("ORKH_USER").expect("ORKH_USER env variable not set");
    let pw = nix::unistd::User::from_name(&user)
        .expect("getpwnam failed")
        .expect("user not found");
    let uid = pw.uid.as_raw();

    let nixos = Path::new("/etc/NIXOS").exists();

    let home = format!("/home/{user}");
    let cfg_dir = format!("{home}/.config/orkh");
    let rundir = format!("/run/user/{uid}");
    let path = if nixos {
        format!("/run/current-system/sw/bin:/etc/profiles/per-user/{user}/bin")
    } else {
        "/usr/bin".to_string()
    };

    let exe = std::env::current_exe().expect("current_exe");
    let exe_s = exe.to_str().expect("non-UTF-8 exe path").to_string();

    fs::create_dir_all(OPENRGB_STATE).expect("create OpenRGB state dir");

    // Env vars to set after --clearenv (bwrap applies env options in order)
    let mut extra_env: Vec<(String, String)> = Vec::new();

    let mut a: Vec<String> = Vec::new();

    // Namespaces and privileges
    push(
        &mut a,
        &[
            "--die-with-parent",
            "--new-session",
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--unshare-cgroup-try",
            "--unshare-net", // private netns; bwrap brings up lo for OpenRGB's SDK port
            "--cap-drop",
            "ALL",
            "--cap-add",
            "CAP_SETUID", // Command::uid() for mmsg/quack
            "--cap-add",
            "CAP_SETGID", // Command::gid() + setgroups(0)
        ],
    );

    // Base system
    if nixos {
        push(
            &mut a,
            &[
                "--ro-bind",
                "/nix/store",
                "/nix/store",
                "--ro-bind",
                "/run/current-system",
                "/run/current-system", // sh, mmsg, ...
                "--ro-bind",
                "/etc/static",
                "/etc/static",
                "--ro-bind-try",
                "/etc/profiles",
                "/etc/profiles", // per-user packages (quack)
                "--ro-bind-try",
                "/etc/udev",
                "/etc/udev",
                "--symlink",
                "/run/current-system/sw/bin/sh",
                "/bin/sh",
            ],
        );
    } else {
        push(
            &mut a,
            &[
                "--ro-bind",
                "/usr",
                "/usr",
                "--symlink",
                "usr/bin",
                "/bin",
                "--symlink",
                "usr/bin",
                "/sbin",
                "--symlink",
                "usr/lib",
                "/lib",
                "--symlink",
                "usr/lib",
                "/lib64",
            ],
        );
    }

    // Minimal /etc: getpwnam also runs inside (monitor_workspaces)
    push(
        &mut a,
        &[
            "--ro-bind",
            "/etc/passwd",
            "/etc/passwd",
            "--ro-bind",
            "/etc/group",
            "/etc/group",
            "--ro-bind",
            "/etc/nsswitch.conf",
            "/etc/nsswitch.conf",
            "--ro-bind-try",
            "/etc/ld.so.cache",
            "/etc/ld.so.cache",
            "--ro-bind-try",
            "/etc/udev",
            "/etc/udev",
        ],
    );

    // Kernel interfaces and scratch space
    push(
        &mut a,
        &[
            "--proc",
            "/proc",
            "--dev",
            "/dev", // minimal: null, zero, urandom, tty, ...
            "--ro-bind",
            "/sys",
            "/sys", // hidapi/libusb enumeration
            "--ro-bind",
            "/run/udev",
            "/run/udev", // libudev device database
            "--tmpfs",
            "/tmp",
        ],
    );

    // Devices (--bind would mount nodev, so device nodes need --dev-bind)
    push(
        &mut a,
        &[
            "--dev-bind",
            "/dev/input",
            "/dev/input",
            "--dev-bind-try",
            "/dev/bus/usb",
            "/dev/bus/usb",
        ],
    );

    // hidraw nodes sit directly in /dev, so bind them one by one.
    // Nodes created after startup (replug) need a restart.
    if let Ok(entries) = fs::read_dir("/dev") {
        for e in entries.flatten() {
            let p = e.path();
            let is_hidraw = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("hidraw"));
            if is_hidraw {
                if let Some(s) = p.to_str() {
                    push(&mut a, &["--dev-bind", s, s]);
                }
            }
        }
    }

    // SMBus/I2C: opt-in, since raw bus access as root is far broader than
    // hidraw (a bad write can corrupt hardware such as RAM SPD EEPROMs).
    if i2c_enabled(&format!("{cfg_dir}/config.yaml")) {
        let mut found = false;
        if let Ok(entries) = fs::read_dir("/dev") {
            for e in entries.flatten() {
                let p = e.path();
                let is_i2c = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("i2c-"));
                if is_i2c {
                    if let Some(s) = p.to_str() {
                        push(&mut a, &["--dev-bind", s, s]);
                        found = true;
                    }
                }
            }
        }
        if !found {
            eprintln!(
                "orkh: openrgb.i2c is enabled but no /dev/i2c-* exist; \
                 load i2c-dev and your SMBus driver (i2c-piix4 on AMD, i2c-i801 on Intel)"
            );
        }
    }

    // Home: empty except the orkh config (directory, so atomic saves still work)
    push(
        &mut a,
        &["--tmpfs", "/home", "--ro-bind", &cfg_dir, &cfg_dir],
    );

    // The config symlink may point outside ~/.config/orkh (dotfiles, stow, ...)
    if let Ok(real) = fs::canonicalize(format!("{cfg_dir}/config.yaml")) {
        if let Some(dir) = real.parent().and_then(Path::to_str) {
            if dir != cfg_dir {
                push(&mut a, &["--ro-bind", dir, dir]);
            }
        }
    }

    // OpenRGB state: the only writable bind
    push(&mut a, &["--bind", OPENRGB_STATE, "/root/.config/OpenRGB"]);

    push(&mut a, &["--tmpfs", &rundir]);

    // mango (mmsg): Wayland compositor socket
    if let Some(wl) = nonempty_env("ORKH_WAYLAND_DISPLAY") {
        let sock = if wl.starts_with('/') {
            wl.clone()
        } else {
            format!("{rundir}/{wl}")
        };
        if Path::new(&sock).exists() {
            push(&mut a, &["--bind", &sock, &sock]);
            extra_env.push(("WAYLAND_DISPLAY".into(), wl));
        } else {
            eprintln!("orkh: wayland socket {sock} not found, skipping");
        }
    }

    // quack
    let duck_dest = format!("{rundir}/duckwm.sock");
    let duck_src = [duck_dest.clone(), format!("/tmp/duckwm-{uid}.sock")]
        .into_iter()
        .find(|p| Path::new(p).exists());
    if let Some(src) = duck_src {
        push(&mut a, &["--bind", &src, &duck_dest]);
    }

    let mut dirs = vec!["/etc", "/run", "/run/user"];
    if nixos {
        dirs.extend(["/nix", "/bin"]);
    }
    for d in dirs {
        push(&mut a, &["--chmod", "0755", d]);
    }

    // The binary itself (dev builds live in target/, outside /usr or /nix/store)
    push(&mut a, &["--ro-bind", &exe_s, &exe_s]);

    push(
        &mut a,
        &[
            "--clearenv",
            "--setenv",
            "PATH",
            &path,
            "--setenv",
            "HOME",
            "/root",
            "--setenv",
            "ORKH_USER",
            &user,
            "--setenv",
            SANDBOXED,
            "1",
        ],
    );
    for (k, v) in &extra_env {
        push(&mut a, &["--setenv", k, v]);
    }
    if let Ok(bt) = std::env::var("RUST_BACKTRACE") {
        push(&mut a, &["--setenv", "RUST_BACKTRACE", &bt]);
    }

    let mut cmd = Command::new(BWRAP);
    cmd.args(&a);

    if cfg!(debug_assertions) && std::env::var_os("ORKH_DEBUG_SHELL").is_some() {
        // Debug builds only: inspect the sandbox from a shell
        cmd.arg("sh");
    } else {
        cmd.arg(&exe).args(std::env::args_os().skip(1));
    }

    let err = cmd.exec(); // only returns on failure
    eprintln!("orkh: failed to exec {BWRAP}: {err}");
    std::process::exit(1);
}
