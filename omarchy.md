# Omarchy integration & quirks (Dell G15 5520)

Everything changed on this Omarchy (Arch/Hyprland) install to make the G15
coexist with `g15`. Two themes: (1) omarchy's defaults write
`dell::kbd_backlight` (SMBIOS), which **wedges the AW-ELC LED controller**
(see [protocol.md](protocol.md)); (2) the G15's Fn keys emit keycodes omarchy
doesn't expect. Use this as the restore checklist after a reinstall.

## 1. SMBIOS writers neutralized (the wedge — critical)

Every path that writes `/sys/class/leds/dell::kbd_backlight` must stay dead:

| Writer | Fix |
|---|---|
| `omarchy-system-lock` → `omarchy-brightness-keyboard off` on every screen lock (**the confirmed wedge trigger**) | Shim at `~/.local/bin/omarchy-brightness-keyboard` reroutes off/restore/cycle to `g15` |
| Omarchy keybinds calling `omarchy-brightness-keyboard` | Same shim (all callers use the bare name) |
| `systemd-backlight` restore at boot | `systemctl mask 'systemd-backlight@leds:dell::kbd_backlight.service'` |
| hypridle idle listener | kbd_backlight block commented out in `~/.config/hypr/hypridle.conf` |
| `/usr/lib/systemd/system-sleep/keyboard-backlight` (omarchy's ASUS hibernate fix) | Guarded with an early exit when `dell::kbd_backlight` exists |

The shim wins over omarchy's copy because `~/.config/uwsm/env` (user config,
survives omarchy updates) ends with:

```bash
export PATH=$HOME/.local/bin:$PATH
```

The omarchy repo (`~/.local/share/omarchy`) is kept pristine so
`omarchy-update` pulls cleanly. Do **not** blacklist `dell_laptop` to remove
the LED node — it also provides the battery charge thresholds (50–90%) in use.

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
- Waybar (`~/.config/waybar/config.jsonc`): `custom/g15` module runs
  `g15 waybar` (reads hwmon + state file only, never opens the USB device);
  click launches the TUI via `omarchy-launch-or-focus-tui g15-tui`.
- `~/.config/hypr/apps.lua`: window rule floats `org.omarchy.g15-tui`.

## 5. Wedge watchdog (dormant)

`~/.local/bin/g15-wedge-watch` + systemd user timer caught the lock-path
trigger, then was disabled 2026-07-14 for false positives. Re-arm when
hunting a new trigger: `systemctl --user enable --now g15-wedge-watch.timer`;
one-shot: run the script and read `~/.local/state/g15-wedge-watch.log`.
Only a *persistent* WEDGED (never flips back OK) is real.
