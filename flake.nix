{
  description = "Flipper: a native GPUI browser and profile picker for Linux";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      runtimeLibraryPath =
        pkgs:
        pkgs.lib.makeLibraryPath (
          with pkgs;
          [
            libxkbcommon
            vulkan-loader
            wayland
          ]
        );
    in
    {
      packages = forAllSystems (pkgs: rec {
        flipper = pkgs.callPackage ./nix/package.nix { };
        firefox-addon = pkgs.callPackage ./nix/firefox-addon.nix { };
        default = flipper;
      });

      # Defaults programs.flipper.package to this flake's build; the module
      # itself falls back to building ./nix/package.nix with the user's pkgs.
      homeModules = rec {
        flipper =
          { lib, pkgs, ... }:
          {
            imports = [ ./nix/hm-module.nix ];
            programs.flipper.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.default;
          };
        default = flipper;
      };

      overlays.default = final: _prev: {
        flipper = final.callPackage ./nix/package.nix { };
      };

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = nixpkgs.lib.getExe self.packages.${pkgs.stdenv.hostPlatform.system}.default;
          meta.description = "Open the Flipper browser picker";
        };
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.default ];
          packages = with pkgs; [
            cargo
            clippy
            rust-analyzer
            rustc
            rustfmt
          ];
          LD_LIBRARY_PATH = runtimeLibraryPath pkgs;
        };
      });

      checks = forAllSystems (pkgs: {
        flipper = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        firefox-addon = self.packages.${pkgs.stdenv.hostPlatform.system}.firefox-addon;
        hm-module = import ./nix/hm-module-check.nix {
          inherit pkgs;
          module = self.homeModules.default;
        };

        rustfmt = pkgs.runCommand "flipper-rustfmt" { nativeBuildInputs = [ pkgs.rustfmt ]; } ''
          rustfmt --check --edition 2024 ${./src}/main.rs
          touch $out
        '';

        nixfmt = pkgs.runCommand "flipper-nixfmt" { nativeBuildInputs = [ pkgs.nixfmt ]; } ''
          nixfmt --check ${./flake.nix} ${./nix}/*.nix
          touch $out
        '';
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
