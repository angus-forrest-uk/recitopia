{
  description = "Recitopia API package and NixOS module";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          rustToolchain = pkgs.rust-bin.stable."1.88.0".default;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
          rustApi = pkgs.callPackage ./apps/api-rs/package.nix {
            inherit rustPlatform;
          };
        in
        {
          recitopia-api = rustApi;
          recitopia-api-rust = rustApi;
          default = rustApi;
        });

      checks = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };
          apiSystem = nixpkgs.lib.nixosSystem {
            inherit system;
            modules = [
              self.nixosModules.recitopia-api
              {
                services.recitopia-api = {
                  enable = true;
                  package = self.packages.${system}.recitopia-api;
                };
                system.stateVersion = "25.11";
              }
            ];
          };
          shadowSystem = nixpkgs.lib.nixosSystem {
            inherit system;
            modules = [
              self.nixosModules.recitopia-api-rust-shadow
              {
                services.recitopia-api-rust-shadow = {
                  enable = true;
                  package = self.packages.${system}.recitopia-api-rust;
                };
                system.stateVersion = "25.11";
              }
            ];
          };
        in
        {
          api-module = pkgs.runCommand "recitopia-api-module-check" {
            apiExecStart = apiSystem.config.systemd.services.recitopia-api.serviceConfig.ExecStart;
            apiStoreMode = apiSystem.config.systemd.services.recitopia-api.environment.RECITOPIA_RUST_STORE_MODE;
          } ''
            case "$apiExecStart" in
              *recitopia-api-rs*) ;;
              *) echo "unexpected ExecStart: $apiExecStart" >&2; exit 1 ;;
            esac
            test "$apiStoreMode" = "read-write"
            touch "$out"
          '';
          rust-shadow-module = pkgs.runCommand "recitopia-rust-shadow-module-check" {
            shadowExecStart = shadowSystem.config.systemd.services.recitopia-api-rust-shadow.serviceConfig.ExecStart;
            shadowPort = shadowSystem.config.systemd.services.recitopia-api-rust-shadow.environment.RECITOPIA_RUST_API_PORT;
          } ''
            test -n "$shadowExecStart"
            test "$shadowPort" = "8079"
            touch "$out"
          '';
        });

      nixosModules.recitopia-api = import ./nix/module.nix;
      nixosModules.recitopia-api-rust-shadow = import ./apps/api-rs/module.nix;
      nixosModules.default = self.nixosModules.recitopia-api;
    };
}
