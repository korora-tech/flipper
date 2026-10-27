# Home Manager module for Flipper.
#
# The generated config.yaml is linked into ~/.config/flipper from the Nix
# store, which puts Flipper in managed mode: the settings window and the
# picker save local changes to ~/.config/flipper/overrides.yaml instead
# (unless allowOverrides is false), and --register, --uninstall and --import
# refuse to run because Home Manager owns mimeapps.list and config.yaml.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  inherit (lib) mkOption types;
  cfg = config.programs.flipper;
  settings = cfg.settings;

  desktopId = "com.korora.Flipper.desktop";
  hostName = "com.korora.flipper";
  firefoxAddonId = "flipper@korora.tech";

  # Mirrors registration.rs.
  mimeTypes = [
    "x-scheme-handler/http"
    "x-scheme-handler/https"
    "text/html"
  ];

  # Chromium user-data directories under $XDG_CONFIG_HOME, mirroring
  # browsers.rs. Each one has its own NativeMessagingHosts directory.
  chromiumDirectories = [
    "google-chrome"
    "google-chrome-beta"
    "google-chrome-unstable"
    "chromium"
    "BraveSoftware/Brave-Browser"
    "microsoft-edge"
    "vivaldi"
    "opera"
  ];

  dropNulls = lib.filterAttrs (_: value: value != null);
  nonBlank = value: value != null && builtins.match "[[:space:]]*" value == null;

  ruleModule = types.submodule {
    options = {
      name = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Label shown in the settings window.";
      };
      domains = mkOption {
        type = types.listOf types.str;
        default = [ ];
        example = [
          "example.com"
          "*.example.org"
        ];
        description = "Hostnames or hostname globs; any may match.";
      };
      paths = mkOption {
        type = types.listOf types.str;
        default = [ ];
        example = [ "/work/*" ];
        description = "URL path globs; any may match.";
      };
      patterns = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Globs matched against the complete URL; any may match.";
      };
      extensions = mkOption {
        type = types.listOf types.str;
        default = [ ];
        example = [
          "pdf"
          "docx"
        ];
        description = "File extensions such as `pdf`, `.pdf` or `ext:pdf`; any may match.";
      };
      action = mkOption {
        type = types.enum [
          "picker"
          "open"
        ];
        default = "picker";
        description = "`open` routes to {option}`profile`; `picker` always shows the picker (an exception rule).";
      };
      profile = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "email:you@example.com";
        description = ''
          Stable profile ID (for example `chrome:Profile 1`,
          `firefox:abcd1234.default-release` or
          `firefox:abcd1234.default-release#container-2`) or
          `email:you@example.com` for a Chromium profile. Required for
          `open` rules and not allowed for `picker` rules.
        '';
      };
    };
  };

  customBrowserModule = types.submodule {
    options = {
      id = mkOption {
        type = types.strMatching "[A-Za-z0-9_-]+";
        description = "Configuration identity; the profile ID becomes `custom:<id>`.";
      };
      name = mkOption {
        type = types.str;
        description = "Browser name shown in the picker.";
      };
      profileName = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Profile label shown in the picker.";
      };
      email = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Account email shown in the picker and matched by `email:` selectors.";
      };
      iconName = mkOption {
        type = types.str;
        default = "web-browser";
        description = "Freedesktop icon name.";
      };
      command = mkOption {
        type = types.nonEmptyListOf types.str;
        example = [
          "firefox"
          "-P"
          "Work"
          "{url}"
        ];
        description = "Executable and arguments, without a shell. `{url}` must be a whole argument and is appended when absent.";
      };
    };
  };

  ruleToYaml =
    rule:
    dropNulls {
      inherit (rule)
        name
        domains
        paths
        patterns
        extensions
        action
        profile
        ;
    };

  customBrowserToYaml =
    browser:
    dropNulls {
      inherit (browser)
        id
        name
        email
        command
        ;
      profile_name = browser.profileName;
      icon_name = browser.iconName;
    };

  # Every key is emitted; defaults match config.rs.
  configData = {
    version = 1;
    appearance = {
      inherit (settings.appearance) theme dim width;
    };
    routing = dropNulls {
      inherit (settings.routing) mode suspended;
      default_profile = settings.routing.defaultProfile;
      unwrap_redirects = settings.routing.unwrapRedirects;
    };
    rules = map ruleToYaml settings.rules;
    browsers = {
      inherit (settings.browsers) hidden order;
    };
    custom_browsers = map customBrowserToYaml settings.customBrowsers;
    overrides.allowed = cfg.allowOverrides;
  };

  # JSON is valid YAML. Validate with Flipper itself so a bad configuration
  # fails the build instead of the next link click.
  configFile =
    pkgs.runCommand "flipper-config.yaml"
      {
        json = builtins.toJSON configData;
        passAsFile = [ "json" ];
      }
      ''
        cp "$jsonPath" "$out"
        ${lib.getExe cfg.package} --check-config "$out"
      '';

  # Mirrors native_host.rs: browsers pass extra arguments that Flipper does
  # not accept, so they are not forwarded.
  nativeHost = pkgs.writeShellScript "flipper-native-host" ''
    exec ${lib.getExe cfg.package} --native-host
  '';

  hostManifest =
    allowed:
    pkgs.writeText "${hostName}.json" (
      builtins.toJSON (
        {
          name = hostName;
          description = "Flipper browser picker";
          path = "${nativeHost}";
          type = "stdio";
        }
        // allowed
      )
    );

  chromiumManifest = hostManifest {
    allowed_origins = map (id: "chrome-extension://${id}/") cfg.nativeMessaging.chromium;
  };
in
{
  options.programs.flipper = {
    enable = lib.mkEnableOption "Flipper, a browser and profile picker";

    package = mkOption {
      type = types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "flipper.packages.\${system}.default";
      description = "The Flipper package.";
    };

    allowOverrides = mkOption {
      type = types.bool;
      default = true;
      description = ''
        Let the settings window and the picker's Always actions save local
        changes to {file}`~/.config/flipper/overrides.yaml`, layered over
        this configuration. When false, overrides are ignored, Flipper's
        interface is read-only, and Always copies a Nix rule instead.
      '';
    };

    defaultBrowser = mkOption {
      type = types.bool;
      default = false;
      description = "Make Flipper the default handler for web links through {option}`xdg.mimeApps`.";
    };

    nativeMessaging = {
      firefox = mkOption {
        type = types.bool;
        default = false;
        description = "Install the native messaging host for the Flipper Firefox add-on (`${firefoxAddonId}`).";
      };
      chromium = mkOption {
        type = types.listOf (types.strMatching "[a-p]{32}");
        default = [ ];
        description = "Chromium extension IDs allowed to use the native messaging host.";
      };
      chromiumDirectories = mkOption {
        type = types.listOf types.str;
        default = chromiumDirectories;
        description = "Chromium user-data directories under {env}`XDG_CONFIG_HOME` that receive the native messaging host manifest.";
      };
    };

    settings = {
      appearance = {
        theme = mkOption {
          type = types.enum [
            "system"
            "light"
            "dark"
          ];
          default = "system";
          description = "Colour theme.";
        };
        dim = mkOption {
          type = types.bool;
          default = true;
          description = "Darken everything behind the link selector.";
        };
        width = mkOption {
          type = types.numbers.between 320 2400;
          default = 760;
          description = "Picker width in logical pixels.";
        };
      };

      routing = {
        mode = mkOption {
          type = types.enum [
            "picker"
            "always"
          ];
          default = "picker";
          description = "What happens when no rule matches: show the picker, or always open {option}`defaultProfile`.";
        };
        defaultProfile = mkOption {
          type = types.nullOr types.str;
          default = null;
          description = "Profile selector used when {option}`mode` is `always`.";
        };
        suspended = mkOption {
          type = types.bool;
          default = false;
          description = "Show the picker for every link without deleting rules.";
        };
        unwrapRedirects = mkOption {
          type = types.bool;
          default = true;
          description = "Resolve Microsoft SafeLinks and Google redirects before routing.";
        };
      };

      rules = mkOption {
        type = types.listOf ruleModule;
        default = [ ];
        description = "Ordered routing rules; the first match wins. Local override rules run before these.";
      };

      browsers = {
        hidden = mkOption {
          type = types.listOf types.str;
          default = [ ];
          description = "Profile or browser IDs to omit from the picker.";
        };
        order = mkOption {
          type = types.listOf types.str;
          default = [ ];
          description = "Profile or browser IDs in preferred display order.";
        };
      };

      customBrowsers = mkOption {
        type = types.listOf customBrowserModule;
        default = [ ];
        description = "Additional launch commands shown as profiles.";
      };
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        assertions =
          lib.imap0 (index: rule: {
            assertion =
              rule.domains != [ ] || rule.paths != [ ] || rule.patterns != [ ] || rule.extensions != [ ];
            message = "programs.flipper.settings.rules.${toString index} needs domains, paths, patterns or extensions; use routing for a default.";
          }) settings.rules
          ++ lib.imap0 (index: rule: {
            assertion = if rule.action == "open" then nonBlank rule.profile else rule.profile == null;
            message =
              if rule.action == "open" then
                "programs.flipper.settings.rules.${toString index}.profile is required for an open action."
              else
                "programs.flipper.settings.rules.${toString index}.profile is only valid for an open action.";
          }) settings.rules
          ++ [
            {
              assertion = settings.routing.mode != "always" || nonBlank settings.routing.defaultProfile;
              message = "programs.flipper.settings.routing.defaultProfile is required when routing.mode is always.";
            }
            {
              assertion =
                let
                  ids = map (browser: browser.id) settings.customBrowsers;
                in
                lib.length ids == lib.length (lib.unique ids);
              message = "programs.flipper.settings.customBrowsers IDs must be unique.";
            }
          ];

        home.packages = [ cfg.package ];
        xdg.configFile."flipper/config.yaml".source = configFile;
      }

      (lib.mkIf cfg.defaultBrowser {
        xdg.mimeApps = {
          enable = true;
          defaultApplications = lib.genAttrs mimeTypes (_: [ desktopId ]);
        };
      })

      (lib.mkIf cfg.nativeMessaging.firefox {
        home.file.".mozilla/native-messaging-hosts/${hostName}.json".source = hostManifest {
          allowed_extensions = [ firefoxAddonId ];
        };
      })

      (lib.mkIf (cfg.nativeMessaging.chromium != [ ]) {
        xdg.configFile = lib.listToAttrs (
          map (
            directory:
            lib.nameValuePair "${directory}/NativeMessagingHosts/${hostName}.json" {
              source = chromiumManifest;
            }
          ) cfg.nativeMessaging.chromiumDirectories
        );
      })
    ]
  );
}
