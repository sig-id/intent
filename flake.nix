{
  description = "Intent - Static analysis tool for design constraint language";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane.url = "github:ipetkov/crane";

    flake-utils.url = "github:numtide/flake-utils";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, crane, flake-utils, rust-overlay, advisory-db, ... }:
    let
      # NixOS VM configuration (x86_64-linux only)
      vmSystem = "x86_64-linux";
      vmPkgs = import nixpkgs {
        system = vmSystem;
        overlays = [ (import rust-overlay) ];
        config.allowUnfree = true;
      };
    in
    {
      # NixOS configuration for VM
      nixosConfigurations.intent-vm = nixpkgs.lib.nixosSystem {
        system = vmSystem;
        specialArgs = { inherit self; };
        modules = [
          ({ config, pkgs, lib, self, ... }:
            let
              rustToolchain = vmPkgs.rust-bin.stable.latest.default;
            in {
              # Basic system configuration
              system.stateVersion = "24.11";

              # Boot configuration for VM
              boot.loader.systemd-boot.enable = true;
              boot.loader.efi.canTouchEfiVariables = true;

              # Filesystems (for VM)
              fileSystems."/" = {
                device = "/dev/disk/by-label/nixos";
                fsType = "ext4";
              };

              # Network
              networking.hostName = "intent-vm";
              networking.networkmanager.enable = true;

              # Enable SSH for VM access
              services.openssh.enable = true;

              # User configuration
              users.users.intent = {
                isNormalUser = true;
                extraGroups = [ "wheel" "networkmanager" ];
                initialPassword = "intent";
                home = "/home/intent";
              };

              # Allow unfree packages
              nixpkgs.config.allowUnfree = true;

              # System packages
              environment.systemPackages = with pkgs; [
                git vim htop curl wget jq
                rustup gcc gnumake pkg-config openssl.dev
                # Language servers
                rust-analyzer
                # Build cache
                sccache
                # Tools
                just
              ];

              # Open SSH port
              networking.firewall.allowedTCPPorts = [ 22 ];

              # VM-specific settings
              virtualisation.vmVariant = {
                virtualisation.memorySize = 8192;  # 8GB
                virtualisation.cores = 4;
                virtualisation.graphics = false;
                virtualisation.diskSize = 20480;  # 20GB disk
                virtualisation.forwardPorts = [
                  { from = "host"; host.port = 2223; guest.port = 22; }  # SSH
                ];
              };

              # Set environment variables for Rust/C builds
              environment.sessionVariables = {
                RUSTC_WRAPPER = "${pkgs.sccache}/bin/sccache";
                SCCACHE_CACHE_SIZE = "5G";
              };
            })
        ];
      };
    } //
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        inherit (pkgs) lib;

        rustToolchain = pkgs.rust-bin.stable.latest.default;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Check if Cargo.lock exists
        cargoLockExists = builtins.pathExists ./Cargo.lock;

        src = if cargoLockExists then craneLib.cleanCargoSource (craneLib.path ./.) else ./.;

        # Common arguments
        commonArgs = {
          inherit src;
          pname = "intent";
          version = "0.1.0";
          strictDeps = true;

          nativeBuildInputs = [
            pkgs.pkg-config
          ];

          buildInputs = [
            pkgs.openssl
          ] ++ lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
          ];
        };

        # Build dependencies only (for caching)
        cargoArtifacts = if cargoLockExists then craneLib.buildDepsOnly commonArgs else null;

        # Build the actual crate
        my-crate = if cargoLockExists then craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
        }) else null;

      in
      {
        checks = lib.optionalAttrs cargoLockExists {
          inherit my-crate;

          my-crate-clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });

          my-crate-doc = craneLib.cargoDoc (commonArgs // {
            inherit cargoArtifacts;
          });

          my-crate-fmt = craneLib.cargoFmt {
            inherit src;
            pname = "intent";
            version = "0.1.0";
          };

          my-crate-audit = craneLib.cargoAudit {
            inherit src advisory-db;
            pname = "intent";
            version = "0.1.0";
          };

          my-crate-deny = craneLib.cargoDeny {
            inherit src;
            pname = "intent";
            version = "0.1.0";
          };

          my-crate-nextest = craneLib.cargoNextest (commonArgs // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
          });
        };

        packages = lib.optionalAttrs cargoLockExists {
          default = my-crate;
        };

        apps = lib.optionalAttrs cargoLockExists {
          default = flake-utils.lib.mkApp {
            drv = my-crate;
          };
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          OPENSSL_DIR = "${pkgs.openssl.dev}";
          OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";

          shellHook = ''
            export RUSTC_WRAPPER="${pkgs.sccache}/bin/sccache"
            export SCCACHE_CACHE_SIZE="5G"
          '';

          packages = with pkgs; [
            pkg-config
            openssl
            openssl.dev
            rust-analyzer
            cargo-watch
            cargo-nextest
            sccache
            just
          ];
        };
      });
}
