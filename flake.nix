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

        # Baked into the binary via option_env
        ORKH_BWRAP = "${pkgs.bubblewrap}/bin/bwrap";
        ORKH_OPENRGB = "${pkgs.openrgb}/bin/openrgb";

        # Build, then run the debug binary as root with the env it needs
        orkhRun = pkgs.writeShellScriptBin "orkh-run" ''
          set -euo pipefail
          cargo build
          exec sudo env \
            ORKH_USER="$USER" \
            ORKH_WAYLAND_DISPLAY="''${WAYLAND_DISPLAY:-wayland-0}" \
            RUST_BACKTRACE=1 \
            "$@" \
            "''${CARGO_TARGET_DIR:-$PWD/target}/debug/orkh"
        '';

        # Same sandbox, but drops you into a shell inside it
        orkhShell = pkgs.writeShellScriptBin "orkh-sandbox-shell" ''
          exec ${orkhRun}/bin/orkh-run ORKH_DEBUG_SHELL=1
        '';
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "orkh";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          inherit ORKH_BWRAP ORKH_OPENRGB;
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
          inherit ORKH_BWRAP ORKH_OPENRGB;
          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      }
    );
}
