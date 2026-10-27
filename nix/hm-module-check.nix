# Evaluate the Home Manager module against a minimal stand-in for the Home
# Manager options it sets, build the generated files (which runs
# `flipper --check-config`), and inspect them. This avoids a home-manager
# flake input that every consumer of this flake would have to fetch.
{ pkgs, module }:
let
  inherit (pkgs) lib;
  inherit (lib) mkOption types;

  fileType = types.attrsOf (
    types.submodule {
      options.source = mkOption { type = types.path; };
    }
  );

  # The subset of Home Manager's options that the module uses.
  homeManagerStub = {
    options = {
      assertions = mkOption {
        type = types.listOf types.unspecified;
        default = [ ];
      };
      home.packages = mkOption {
        type = types.listOf types.package;
        default = [ ];
      };
      home.file = mkOption {
        type = fileType;
        default = { };
      };
      xdg.configFile = mkOption {
        type = fileType;
        default = { };
      };
      xdg.mimeApps = {
        enable = mkOption {
          type = types.bool;
          default = false;
        };
        defaultApplications = mkOption {
          type = types.attrsOf (types.listOf types.str);
          default = { };
        };
      };
    };
  };

  evaluate =
    flipper:
    (lib.evalModules {
      modules = [
        homeManagerStub
        module
        {
          _module.args.pkgs = pkgs;
          programs.flipper = flipper;
        }
      ];
    }).config;

  example = evaluate {
    enable = true;
    defaultBrowser = true;
    allowOverrides = false;
    nativeMessaging = {
      firefox = true;
      chromium = [ "abcdefghijklmnopabcdefghijklmnop" ];
    };
    settings = {
      appearance.theme = "dark";
      routing = {
        mode = "always";
        defaultProfile = "firefox:abcd1234.default-release";
      };
      rules = [
        {
          name = "Admin";
          domains = [ "example.com" ];
          paths = [ "/admin" ];
        }
        {
          name = "Work";
          domains = [ "example.com" ];
          action = "open";
          profile = "email:you@example.com";
        }
        {
          domains = [ "github.com" ];
          action = "open";
          profile = "firefox:abcd1234.default-release#container-2";
        }
        {
          extensions = [ "pdf" ];
        }
      ];
      browsers = {
        hidden = [ "opera" ];
        order = [ "firefox" ];
      };
      customBrowsers = [
        {
          id = "work";
          name = "Work Firefox";
          command = [
            "firefox"
            "-P"
            "Work"
            "{url}"
          ];
        }
      ];
    };
  };

  failedAssertions =
    config:
    map (assertion: assertion.message) (lib.filter (assertion: !assertion.assertion) config.assertions);

  invalid = evaluate {
    enable = true;
    settings.rules = [
      {
        domains = [ "example.com" ];
        action = "open";
      }
      {
        domains = [ "example.org" ];
        profile = "chrome:Default";
      }
    ];
  };

  expect = condition: message: if condition then true else throw "hm-module check: ${message}";

  evaluationChecks =
    assert expect (
      failedAssertions example == [ ]
    ) "example has failed assertions: ${toString (failedAssertions example)}";
    assert expect (lib.length (failedAssertions invalid) == 2) "invalid rules were not rejected";
    assert expect (
      example.xdg.mimeApps.defaultApplications."x-scheme-handler/https"
      == [ "com.korora.Flipper.desktop" ]
    ) "defaultBrowser did not set mimeApps";
    assert expect (
      example.xdg.configFile ? "BraveSoftware/Brave-Browser/NativeMessagingHosts/com.korora.flipper.json"
    ) "Chromium native messaging manifests are missing";
    assert expect (
      (evaluate { enable = false; }).home.packages == [ ]
    ) "disabled module changed the configuration";
    true;

  configYaml = example.xdg.configFile."flipper/config.yaml".source;
  firefoxManifest =
    example.home.file.".mozilla/native-messaging-hosts/com.korora.flipper.json".source;
  chromeManifest =
    example.xdg.configFile."google-chrome/NativeMessagingHosts/com.korora.flipper.json".source;
  flipper = lib.getExe example.programs.flipper.package;
in
assert evaluationChecks;
pkgs.runCommand "flipper-hm-module-check"
  {
    nativeBuildInputs = [ pkgs.jq ];
  }
  ''
    jq -e '
      .version == 1
      and .overrides.allowed == false
      and .appearance.theme == "dark"
      and .routing.default_profile == "firefox:abcd1234.default-release"
      and .rules[0].action == "picker"
      and (.rules[0] | has("profile") | not)
      and .rules[1].profile == "email:you@example.com"
      and .custom_browsers[0].icon_name == "web-browser"
      and (.custom_browsers[0] | has("profile_name") | not)
    ' ${configYaml}
    jq -e '.allowed_extensions == ["flipper@korora.tech"] and .type == "stdio"' ${firefoxManifest}
    jq -e '.allowed_origins == ["chrome-extension://abcdefghijklmnopabcdefghijklmnop/"]' ${chromeManifest}
    host=$(jq -r .path ${firefoxManifest})
    grep -q -- '--native-host' "$host"

    # Shape errors that the module's assertions do not cover still fail.
    printf 'version: 1\ncustom_browsers:\n  - id: x\n    name: X\n    command: [browser, "--url={url}"]\n' > bad.yaml
    if ${flipper} --check-config bad.yaml 2> error.txt; then
      echo "--check-config accepted an invalid configuration" >&2
      exit 1
    fi
    grep -q 'must be a complete argument' error.txt

    touch $out
  ''
