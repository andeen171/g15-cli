# Omarchy integration & quirks (Dell G15 5520)

Everything changed on this Omarchy (Arch/Hyprland) install to make the G15
coexist with `g15`. Two themes: (1) omarchy's defaults write
`dell::kbd_backlight` (SMBIOS), which **wedges the AW-ELC LED controller**
(see [protocol.md](protocol.md)); (2) the G15's Fn keys emit keycodes omarchy
doesn't expect. Use this as the restore checklist after a reinstall.

Written against **Omarchy 4** (`omarchy-dev 4.0.0`), which moved the bar,
idle, and lock into one Quickshell process (`omarchy-shell`), replaced waybar
and hypridle, made `/usr/share/omarchy` package-owned (`~/.local/share/omarchy`
is now just a symlink to it), and dropped `~/.config/uwsm/env`.

## 1. SMBIOS writers neutralized (the wedge — critical)

Every path that writes `/sys/class/leds/dell::kbd_backlight` must stay dead:

| Writer | Fix |
|---|---|
| `omarchy-brightness-keyboard off` on every screen lock (**the confirmed wedge trigger**) — in Omarchy 3 from `omarchy-system-lock`, in Omarchy 4 from the shell's lock plugin, 5 s after the lock screen goes idle (`/usr/share/omarchy/shell/plugins/lock/Service.qml`, `blankProcess`) | Shim at `~/.local/bin/omarchy-brightness-keyboard` reroutes off/restore/cycle to `g15` — **but see the PATH gap below: the shim does not cover the Omarchy 4 lock path** |
| Omarchy keybinds calling `omarchy-brightness-keyboard` | Rebound to `g15` by absolute path in `bindings.lua` (§3), so PATH order is irrelevant |
| `systemd-backlight` restore at boot | `systemctl mask 'systemd-backlight@leds:dell::kbd_backlight.service'` |
| hypridle idle listener | Moot on Omarchy 4 (hypridle is gone; idle is a shell service). The kbd_backlight block stays commented out in `~/.config/hypr/hypridle.conf` for a downgrade/reinstall |
| `/usr/lib/systemd/system-sleep/keyboard-backlight` (omarchy's ASUS hibernate fix) | Guarded with an early exit when `dell::kbd_backlight` exists |

**PATH gap (Omarchy 4, open).** The shim only wins for callers that resolve the
bare name with `~/.local/bin` first. `omarchy-shell` is launched with
`PATH=/usr/share/omarchy/bin:...`, so *its* children — including the lock
blank — get the packaged binary and write SMBIOS:

```sh
# what the shell would actually run
tr '\0' '\n' < /proc/$(pgrep -f quickshell | head -1)/environ | sed -n 's/^PATH=//p'
```

Not yet observed firing (no `brightnessctl` save state under
`$XDG_RUNTIME_DIR/brightnessctl` or `/var/lib/brightnessctl` after a day of
uptime, and `g15 info` still reads firmware 1.1.12), so it is a live hazard
rather than a known break. `~/.config/uwsm/env` no longer exists to reorder
PATH; the supported fix is `omarchy plugin clone omarchy.lock` and dropping
`omarchy-brightness-keyboard off` from `blankProcess` in the clone.

`/usr/share/omarchy` is package-owned — never edit it (it is also what
`~/.local/share/omarchy` now points at). Do **not** blacklist `dell_laptop` to
remove the LED node — it also provides the battery charge thresholds (50–90%)
in use.

## 2. Internal keyboard keycodes (`/etc/udev/hwdb.d/61-g15-keyboard.hwdb`)

```
evdev:atkbd:dmi:bvn*:bvr*:bd*:svnDellInc.:pnDellG155520:*
 KEYBOARD_KEY_68=prog1
 KEYBOARD_KEY_73=slash
```

Apply with `sudo systemd-hwdb update && sudo udevadm trigger
/dev/input/by-path/platform-i8042-serio-0-event-kbd`.

- **0x68 (Fn+F9, G-Mode key):** upstream hwdb maps it to `KEY_PERFORMANCE`
  (keycode 701) — above XKB's keysym range, so it's unbindable by keysym, and
  hyprlua fails to parse `code:709` too: it registers `key="" keycode=0`,
  which Hyprland treats as a **NoSymbol wildcard** that fires on *any* key
  without a keysym in the current layout. On this ABNT2 keyboard with a `us`
  layout that included the `?/` key next to right shift (KEY_RO, unmapped) —
  it triggered G-Mode instead of typing. Remapping to `prog1` makes the key a
  normal `XF86Launch1`.
- **0x73 (the ABNT2 `?/` key):** `KEY_RO` has no keysym in the `us` layout;
  remapped to `slash` so it types `/` (Shift = `?`), matching the keycap.

## 3. Hyprland bindings (`~/.config/hypr/bindings.lua`)

- Omarchy's `XF86KbdLightOnOff` / `XF86KbdBrightnessUp/Down` defaults are
  `hl.unbind`-ed (they call the SMBIOS path) and rebound to
  `g15 led brightness cycle` (`locked = true` so they work on the lockscreen).
- **Fn+F5** emits `KEY_F18` (Dell routes kbd-illumination to AWCC in software
  on Windows); XKB presents it as `XF86Launch9` → bound to the same cycle.
- **Fn+F9** (after the hwdb remap above) is `XF86Launch1` → runs
  `sudo -n g15 power toggle` + notify. Needs the NOPASSWD entry in
  `/etc/sudoers.d/g15-power-toggle`.

## 4. Desktop integration

- `~/.config/hypr/autostart.lua`: `g15 restore` reapplies the saved LED
  state at session start (over USB — never sysfs).
- Bar widget: Omarchy 4 replaced waybar with the Quickshell `omarchy-shell`, so
  the module is now the `io.github.andeen171.g15` **plugin** from
  `omarchy-plugin/` in this repo, copied to
  `~/.config/omarchy/plugins/io.github.andeen171.g15` and enabled in the bar's
  left section (see README → Bar module). It polls `g15 status` (hwmon + state
  file only, never the USB device) every 5 s; left click opens a panel with
  temperature meters, power-mode chips, a fan-boost slider and the full
  backlight controls (brightness, effect, speed, color list, HSV picker), right
  click launches the TUI via `omarchy-launch-or-focus-tui g15-tui`.
  Editing the plugin's QML needs `omarchy restart shell`; the shell does not
  hot-reload a loaded plugin component, and neither `rescanPlugins` nor a
  disable/enable cycle rebuilds it.
  The power chips and the boost slider shell out through `sudo -n g15 …` and
  stay inert without `/etc/sudoers.d/` entries for `/usr/bin/g15 power *` and
  `/usr/bin/g15 fan boost *` — the original `g15-power-toggle` entry only
  covered `power toggle` (the Fn+F9 bind).
  Predecessors, both still working as fallbacks: the `andeen171.g15` plugin
  (text label + click-to-TUI, superseded 2026-08-26) and an inline
  `{"id":"g15","type":"command",...}` entry in `~/.config/omarchy/shell.json`.
- `~/.local/bin/g15-tui`: wrapper the click target runs (`exec sudo g15 tui`).
- `~/.config/hypr/apps.lua`: window rule floats `org.omarchy.g15-tui`.

Reinstall note: plugins live in `~/.config`, so the AUR package cannot ship the
widget — the copy step above is part of the restore checklist.

## 5. Wedge watchdog (dormant)

`~/.local/bin/g15-wedge-watch` + systemd user timer caught the lock-path
trigger, then was disabled 2026-07-14 for false positives. Re-arm when
hunting a new trigger: `systemctl --user enable --now g15-wedge-watch.timer`;
one-shot: run the script and read `~/.local/state/g15-wedge-watch.log`.
Only a *persistent* WEDGED (never flips back OK) is real.
