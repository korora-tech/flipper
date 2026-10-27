# The Flipper Firefox add-on as an unpacked-install XPI, laid out for
# Home Manager's programs.firefox.profiles.<name>.extensions.packages.
{
  lib,
  runCommand,
  zip,
}:
let
  addonId = "flipper@korora.tech";
  # Firefox's application ID; extensions installed under it are sideloaded.
  firefoxGuid = "{ec8030f7-c20a-464f-9b0e-13a3a9e97384}";
  manifest = lib.importJSON ../extensions/firefox/manifest.json;
  source = lib.fileset.toSource {
    root = ../extensions/firefox;
    fileset = ../extensions/firefox;
  };
in
runCommand "flipper-firefox-addon-${manifest.version}"
  {
    nativeBuildInputs = [ zip ];
    passthru = { inherit addonId; };
    meta = {
      description = "Firefox add-on for Flipper's context menu and container hand-off";
      homepage = "https://github.com/korora-tech/flipper";
      license = lib.licenses.mit;
      platforms = lib.platforms.all;
    };
  }
  ''
    install -d "$out/share/mozilla/extensions/${firefoxGuid}"
    cd ${source}
    zip -q -r -X "$out/share/mozilla/extensions/${firefoxGuid}/${addonId}.xpi" .
  ''
