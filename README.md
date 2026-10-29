# Flipper

Flipper is a Linux browser and profile picker built with GPUI and Wayland
layer-shell. It can sit between desktop links and browsers, identify browser
profiles, and apply local YAML routing rules.

## Features

Flipper uses a GPUI interface and YAML configuration.

| Feature | Behavior | Access |
| --- | --- | --- |
| Default-browser handoff and restore | Register and restore the user's previous MIME handlers | `flipper --register`, `flipper --uninstall` |
| Browser/profile picker | Searchable profile cards with icons, account email, number shortcuts, Escape | Picker |
| Chromium profile detection | Chrome, Edge, Brave, Vivaldi, Opera, Chromium; include native and sandboxed data roots | Auto-detected |
| Firefox profile detection | Firefox (Beta, Developer Edition, Nightly, ESR), Zen, Floorp, LibreWolf, Waterfox, Mullvad; installed browsers only; Firefox profiles in `~/.mozilla/firefox` or `$XDG_CONFIG_HOME/mozilla/firefox` | Auto-detected |
| Email profile selection | Chrome Local State `profile.info_cache.*.user_name`; exact, unique matches only | YAML rule selector and picker labels |
| Always and once | Open once or save a domain/path rule for future links | Picker actions |
| Rules | Domain, path, wildcard, exceptions, order, fallback and temporary suspension | YAML and settings |
| Redirect unwrapping | Nested Microsoft SafeLinks and Google redirect URLs | Before routing |
| Destination cues | Show the effective destination and Office document type | Picker |
| Browser management | Hide, reorder and add custom launch commands | YAML |
| Settings and manual open | Routing, browsers, rules, appearance; open an arbitrary URL | `flipper` or `flipper --settings` |
| Import/export | Validate and atomically import/export YAML | CLI |
| Home Manager | Declarative configuration, default browser and native hosts, with local overrides | `homeModules.default` |
| Safety check | Explicitly open Google's Transparency Report for the destination | Picker action |
| Context-menu extension | Chrome and Firefox extensions backed by a native messaging host | `extensions/` |
| Firefox containers | Pick a container per link; Multi-Account Containers site assignments still apply | Picker, with the Firefox extension installed in that profile |

The implementation is split into a GPUI front end and platform-independent
configuration, discovery, URL preparation, routing, and launch modules. It
does not keep link history or send telemetry. Chrome account labels are read
from Chromium's local `Local State` profile metadata only, and Firefox
containers from a profile's `containers.json` when the Flipper extension is
installed there; cookies, token stores, history, and browsing databases are
not read. An email selector must
match exactly one profile; duplicate or missing emails fall back to the picker.

## Layer-shell support

The link selector uses a GPUI layer-shell surface on Wayland compositors that
support the protocol. Running `flipper` without a URL opens settings in a
normal application window.
On X11, and when a Wayland compositor rejects layer-shell, the link selector
falls back to a normal GPUI window. Compositor focus and placement still need
verification in a real session.

## Configuration

The config path is `$XDG_CONFIG_HOME/flipper/config.yaml`, falling back to
`~/.config/flipper/config.yaml`. Flipper ships with defaults; an example is in
`config.example.yaml`. Rules run in order; values within a condition field are
alternatives, and populated fields are combined. `action: picker` is an
exception rule. `profile` accepts a stable profile ID or
`email:you@example.com`.

Validate a file without touching your configuration with
`flipper --check-config PATH`. It checks shape only: profile selectors are not
resolved, because the browsers they name may not exist where the file is built.

### Profile IDs

Profile IDs are shown in the settings window under Browsers.

| Source | ID | Example |
| --- | --- | --- |
| Chromium browsers | `<browser>:<profile directory>` | `chrome:Default`, `chrome:Profile 1`, `brave:Profile 2` |
| Chromium with `CHROME_USER_DATA_DIR` or `CHROME_CONFIG_HOME` | `<browser>@<root hash>:<profile directory>` | `chrome@0123456789abcdef:Default` |
| Firefox-family browsers | `<browser>:<profiles.ini Path>` | `firefox:abcd1234.default-release`, `zen:xyz.Default (release)` |
| Firefox containers | `<profile ID>#container-<userContextId>` | `firefox:abcd1234.default-release#container-2` |
| Custom browsers | `custom:<id>` | `custom:work` |

Flatpak and Snap installs add `-flatpak` or `-snap` to the browser ID, as in
`firefox-flatpak:abcd1234.default`. Firefox editions share Mozilla's profile
root, so a profile appears once per installed edition (`firefox:…`,
`firefox-nightly:…`). Container IDs are the `userContextId` values from the
profile's `containers.json`; Firefox's built-in containers are normally 1
(Personal), 2 (Work), 3 (Banking) and 4 (Shopping), and containers you create
get higher numbers. Containers are listed only for profiles where the Flipper
add-on is installed as `extensions/flipper@korora.tech.xpi`.

`email:you@example.com` matches the signed-in account in Chromium's
`Local State` (and a custom browser's `email`), case-insensitively; Firefox
profiles have no email. The address must belong to exactly one profile,
otherwise the link falls back to the picker. `browsers.hidden` and
`browsers.order` also accept browser IDs such as `chrome` or `firefox`.

### Local overrides

When `config.yaml` is a symlink — as when Home Manager links it from the Nix
store — Flipper treats it as managed and never writes it. Changes made in the
settings window, with the picker's Always and Always ask actions, or with
`--suspend` and `--resume` are saved to `overrides.yaml` in the same directory
instead. `--register`, `--uninstall` and `--import` refuse to run, because Home
Manager owns `mimeapps.list` and `config.yaml`.

`overrides.yaml` uses the same version and keys as `config.yaml`, except that
every field is optional and `overrides` is not allowed:

```yaml
version: 1
routing:
  suspended: true
rules:
  - name: Personal GitHub
    domains: [github.com]
    action: open
    profile: firefox:abcd1234.default-release#container-1
```

It is layered over `config.yaml` whenever it exists:

| Key | Merge |
| --- | --- |
| `rules` | Override rules run before every base rule |
| `appearance.*`, `routing.*` | A field set in the overrides wins |
| `browsers.hidden` | Union |
| `browsers.order` | Override entries first, then the remaining base order |
| `custom_browsers` | Merged by `id`; the override wins |

If `overrides.yaml` is invalid, or makes the merged configuration invalid, it
is ignored with a warning. Without a managed `config.yaml`, edits still go to
`config.yaml` and remove any override that would hide them. A managed
configuration can disable overrides with:

```yaml
overrides:
  allowed: false
```

Flipper then ignores `overrides.yaml`, the settings window is read-only, and
the picker's Always actions copy the rule as a Nix snippet instead. The
settings window marks managed rules with a Home Manager tag; local rules can be
removed, and a page's reset button restores the managed value of overridden
settings. Promote local changes into Nix with
`flipper --export-overrides --format nix`, or print them as YAML with
`flipper --export-overrides`.

## Home Manager

The flake exports `homeModules.default` (also `homeModules.flipper`). The
generated `config.yaml` is checked with `flipper --check-config` when it is
built, so an invalid configuration fails `home-manager switch`.

```nix
{
  inputs.flipper.url = "github:korora-tech/flipper";

  # In your Home Manager configuration:
  imports = [ inputs.flipper.homeModules.default ];

  programs.flipper = {
    enable = true;
    # Set xdg.mimeApps defaults for http, https and text/html.
    defaultBrowser = true;
    # Let the settings window and picker save overrides.yaml (default).
    allowOverrides = true;
    nativeMessaging = {
      firefox = true;
      chromium = [ "abcdefghijklmnopabcdefghijklmnop" ];
    };
    settings = {
      appearance = {
        theme = "dark"; # system, light or dark
        dim = true;
        width = 760;
      };
      routing = {
        mode = "picker"; # or "always" with defaultProfile
        defaultProfile = null;
        suspended = false;
        unwrapRedirects = true;
      };
      rules = [
        {
          name = "Choose for admin";
          domains = [ "example.com" ];
          paths = [ "/admin" ];
          action = "picker";
        }
        {
          name = "Company links";
          domains = [ "example.com" "*.example.org" ];
          action = "open";
          profile = "email:you@example.com";
        }
        {
          name = "GitHub in the Work container";
          domains = [ "github.com" ];
          action = "open";
          profile = "firefox:abcd1234.default-release#container-2";
        }
        {
          name = "Documents";
          extensions = [ "pdf" "docx" ];
        }
      ];
      browsers = {
        hidden = [ "opera" ];
        order = [ "firefox" "chrome:Profile 1" ];
      };
      customBrowsers = [
        {
          id = "work";
          name = "Firefox";
          profileName = "Work";
          command = [ "firefox" "-P" "Work" "{url}" ];
        }
      ];
    };
  };

  # Install the add-on for context menus and containers.
  programs.firefox.profiles.default.extensions.packages = [
    inputs.flipper.packages.${pkgs.stdenv.hostPlatform.system}.firefox-addon
  ];
}
```

Option names are the YAML keys in camelCase (`default_profile` becomes
`defaultProfile`, `custom_browsers` becomes `customBrowsers`). Rules keep their
order. `nativeMessaging.firefox` installs the host manifest for the add-on
`flipper@korora.tech`; `nativeMessaging.chromium` installs one for the listed
extension IDs in each Chromium browser's `NativeMessagingHosts` directory under
`$XDG_CONFIG_HOME` (set `nativeMessaging.chromiumDirectories` to narrow it). If
another module also sets the web MIME defaults, use `lib.mkForce` on
`xdg.mimeApps.defaultApplications` for the conflicting types.

`packages.firefox-addon` is the add-on as
`share/mozilla/extensions/{ec8030f7-c20a-464f-9b0e-13a3a9e97384}/flipper@korora.tech.xpi`,
with `passthru.addonId = "flipper@korora.tech"`.

## Browser context menus

The extension sources are in `extensions/chrome` and `extensions/firefox`.
Load the extension, then run `flipper --install-native-host chrome EXTENSION_ID`
or `flipper --install-native-host firefox flipper@korora.tech`. Chrome's ID is
shown at `chrome://extensions` after loading the unpacked extension.

Firefox cannot open a container from the command line, so the Firefox
extension also handles `ext+flipper:` links: Flipper launches Firefox with one,
and the extension opens the URL in the chosen container. Container choices
appear only for profiles where the extension is installed.

## Build

```sh
nix build
```

The Nix package is exposed as `.#flipper` and through the default package.

Set Flipper as the desktop browser with `flipper --register`; `--uninstall`
restores previous handlers that still point at Flipper. Import or export YAML
with `--import PATH` and `--export [PATH|-]`; `--export` writes the effective
configuration, including local overrides.

## Website

The site at <https://flipper.korora.tech> is in `website/`: an Astro and
Starlight build deployed to Cloudflare. See `website/README.md`.
