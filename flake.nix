{
  description = "Flipper: a native GPUI browser and profile picker for Linux";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      runtimeLibraryPath = pkgs: pkgs.lib.makeLibraryPath (with pkgs; [ libxkbcommon vulkan-loader wayland ]);
    in {
      packages = forAllSystems (pkgs: rec {
        flipper = pkgs.callPackage ./nix/package.nix { };
        default = flipper;
      });

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
          packages = with pkgs; [ cargo clippy rust-analyzer rustc rustfmt ];
          LD_LIBRARY_PATH = runtimeLibraryPath pkgs;
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
