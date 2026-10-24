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
| Firefox profile detection | Firefox, Zen, Floorp, LibreWolf, Waterfox, Mullvad | Auto-detected |
| Email profile selection | Chrome Local State `profile.info_cache.*.user_name`; exact, unique matches only | YAML rule selector and picker labels |
| Always and once | Open once or save a domain/path rule for future links | Picker actions |
| Rules | Domain, path, wildcard, exceptions, order, fallback and temporary suspension | YAML and settings |
| Redirect unwrapping | Nested Microsoft SafeLinks and Google redirect URLs | Before routing |
| Destination cues | Show the effective destination and Office document type | Picker |
| Browser management | Hide, reorder and add custom launch commands | YAML |
| Settings and manual open | Inspect/edit config and open an arbitrary URL | Settings/Open view |
| Import/export | Validate and atomically import/export YAML | CLI |
| Safety check | Explicitly open Google's Transparency Report for the destination | Picker action |
| Context-menu extension | Chrome and Firefox extensions backed by a native messaging host | `extensions/` |
| Firefox containers | Let Firefox's own Multi-Account Containers site assignments continue to apply | Native Firefox behavior |

The implementation is split into a GPUI front end and platform-independent
configuration, discovery, URL preparation, routing, and launch modules. It
does not keep link history or send telemetry. Chrome account labels are read
from Chromium's local `Local State` profile metadata only; cookies, token
stores, history, and browsing databases are not read. An email selector must
match exactly one profile; duplicate or missing emails fall back to the picker.

## Layer-shell support

The link selector uses a GPUI layer-shell surface on Wayland compositors that
support the protocol. The main app and settings open as normal GPUI windows.
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

## Browser context menus

The extension sources are in `extensions/chrome` and `extensions/firefox`.
Load the extension, then run `flipper --install-native-host chrome EXTENSION_ID`
or `flipper --install-native-host firefox flipper@korora.tech`. Chrome's ID is
shown at `chrome://extensions` after loading the unpacked extension.

## Build

```sh
nix build
```

The Nix package is exposed as `.#flipper` and through the default package.

Set Flipper as the desktop browser with `flipper --register`; `--uninstall`
restores previous handlers that still point at Flipper. Import or export YAML
with `--import PATH` and `--export [PATH|-]`.
