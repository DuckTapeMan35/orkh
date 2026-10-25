{
  description = "orkh: modifier-aware keyboard RGB";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        # Baked into the binary via option_env!
        ORKH_BWRAP = "${pkgs.bubblewrap}/bin/bwrap";
        ORKH_OPENRGB = "${pkgs.openrgb}/bin/openrgb";
        ORKH_SHELL = "${pkgs.bashInteractive}/bin/bash";

        orkhRun = pkgs.writeShellScriptBin "orkh-run" ''
          set -euo pipefail
          cargo build
          exec sudo env \
            ORKH_USER="$USER" \
            RUST_BACKTRACE=1 \
            "$@" \
            "''${CARGO_TARGET_DIR:-$PWD/target}/debug/orkh"
        '';

        orkhShell = pkgs.writeShellScriptBin "orkh-sandbox-shell" ''
          exec ${orkhRun}/bin/orkh-run ORKH_DEBUG_SHELL=1
        '';
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "orkh";
          version = "0.1.0";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
            # Git dependencies aren't hashed in Cargo.lock: build once, paste the hash.
            outputHashes."duckwm-0.1.0" = "sha256-scCh9lL3i/2QucIkWyw3xw68Z9izFJ0SdVnyc+O1Bbs=";
          };
          inherit ORKH_BWRAP ORKH_OPENRGB ORKH_SHELL;
          meta.mainProgram = "orkh";
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer
            bubblewrap
            openrgb
            strace
            orkhRun
            orkhShell
          ];
          inherit ORKH_BWRAP ORKH_OPENRGB ORKH_SHELL;
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      }
    )
    // {
      nixosModules.default =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        let
          cfg = config.services.orkh;
          uid = toString cfg.uid;
          rundir = "/run/user/${uid}";
        in
        {
          options.services.orkh = {
            enable = lib.mkEnableOption "orkh keyboard highlighter";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
              description = "The orkh package to use.";
            };

            user = lib.mkOption {
              type = lib.types.str;
              description = "User whose config (~/.config/orkh) and duckwm orkh follows.";
            };

            uid = lib.mkOption {
              type = lib.types.nullOr lib.types.int;
              default = config.users.users.${cfg.user}.uid;
              defaultText = lib.literalExpression "config.users.users.\${user}.uid";
              description = "UID of that user; needed to find /run/user/<uid>/duckwm.sock.";
            };

            i2c = {
              enable = lib.mkEnableOption "SMBus/I2C access for RGB RAM, motherboard and GPU lighting";
              driver = lib.mkOption {
                type = lib.types.nullOr lib.types.str;
                default = null;
                example = "i2c-piix4";
                description = "SMBus driver to load: i2c-piix4 on AMD, i2c-i801 on Intel.";
              };
            };
          };

          config = lib.mkIf cfg.enable {
            assertions = [
              {
                assertion = cfg.uid != null;
                message = "services.orkh: set users.users.${cfg.user}.uid or services.orkh.uid.";
              }
              {
                assertion = !(config.services.hardware.openrgb.enable or false);
                message = "services.orkh runs its own OpenRGB; disable services.hardware.openrgb.";
              }
            ];

            services.udev.packages = [ pkgs.openrgb ];

            hardware.i2c.enable = lib.mkIf cfg.i2c.enable true;
            boot.kernelModules = lib.optional (cfg.i2c.enable && cfg.i2c.driver != null) cfg.i2c.driver;

            systemd.services.orkh = {
              description = "orkh keyboard highlighter";
              wantedBy = [ "multi-user.target" ];
              environment = {
                ORKH_USER = cfg.user;
              }
              // lib.optionalAttrs cfg.i2c.enable {
                ORKH_I2C = "1";
              };
              serviceConfig = {
                ExecStart = lib.getExe cfg.package;
                StateDirectory = "orkh";
                Restart = "on-failure";
                RestartSec = 5;
                # Hardening that doesn't interfere with bwrap; the sandbox does the rest
                NoNewPrivileges = true;
                LockPersonality = true;
                RestrictRealtime = true;
                SystemCallArchitectures = "native";
              };
            };

            # The sandbox binds duckwm's socket once at startup: restart orkh when it appears
            systemd.paths.orkh-rebind = {
              wantedBy = [ "multi-user.target" ];
              pathConfig = {
                PathExists = [
                  "${rundir}/duckwm.sock"
                  "/tmp/duckwm-${uid}.sock"
                ];
                Unit = "orkh-rebind.service";
              };
            };

            # Tied to the runtime dir, so it resets on logout and fires again on login
            systemd.services.orkh-rebind = {
              description = "Restart orkh to bind duckwm's socket";
              bindsTo = [ "user-runtime-dir@${uid}.service" ];
              after = [ "user-runtime-dir@${uid}.service" ];
              serviceConfig = {
                Type = "oneshot";
                RemainAfterExit = true;
                ExecStart = "${config.systemd.package}/bin/systemctl try-restart orkh.service";
              };
            };
          };
        };
    };
}
