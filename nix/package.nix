{
  lib,
  rustPlatform,
  makeWrapper,
  xdg-utils,
  pkg-config,
  fontconfig,
  freetype,
  libxcb,
  libxkbcommon,
  vulkan-loader,
  vulkan-headers,
  wayland,
}:
let
  cargo = lib.importTOML ../Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = cargo.package.name;
  inherit (cargo.package) version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../data
      ../extensions
      ../config.example.yaml
      ../LICENSE
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;
  doCheck = false;

  nativeBuildInputs = [ makeWrapper pkg-config ];
  buildInputs = [
    fontconfig
    freetype
    libxcb
    libxkbcommon
    vulkan-loader
    vulkan-headers
    wayland
  ];

  postInstall = ''
    install -Dm644 ${../data/com.korora.Flipper.desktop} \
      $out/share/applications/com.korora.Flipper.desktop
    install -Dm644 ${../config.example.yaml} \
      $out/share/flipper/config.example.yaml
    cp -r ${../extensions} $out/share/flipper/extensions
  '';

  postFixup = ''
    wrapProgram $out/bin/flipper \
      --prefix PATH : ${lib.makeBinPath [ xdg-utils ]} \
      --prefix LD_LIBRARY_PATH : ${
        lib.makeLibraryPath [ libxkbcommon vulkan-loader wayland ]
      }
  '';

  meta = {
    description = "A native GPUI browser and profile picker for Linux";
    homepage = "https://github.com/korora-tech/flipper";
    license = lib.licenses.mit;
    mainProgram = "flipper";
    platforms = lib.platforms.linux;
  };
}
