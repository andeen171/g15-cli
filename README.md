# g15

Keyboard backlight, fan, and power-mode control for the Dell G15 5520 on Linux
(likely works on other AW-ELC G-series models — 5511/5515/5525 — untested).
Reverse-engineered from Alienware Command Center's USB traffic — see
[protocol.md](protocol.md) for the full AW-ELC (187c:0550) protocol writeup.

## Why another tool

Existing tools (dell-g-series-controller, OpenRGB) didn't work on this firmware.
Four reasons, all documented in protocol.md:

1. The firmware only answers on the USB **control pipe** — hidraw `write()`
   (what everyone uses) goes to dead interrupt endpoints.
2. The color zones are `0x10-0x13`, not `0-3`.
3. The `0x26` command is *dimming*, inverted: `0` = full brightness, `100` = off.
4. Writes to `/sys/class/leds/dell::kbd_backlight` (Dell SMBIOS) **hard-wedge
   the LED controller** until you cut all power (30 s power button with AC and
   battery removed). See [Protecting the controller](#protecting-the-controller).

## Install

### AUR

```sh
yay -S g15-cli
```

### From source

```sh
cargo install --path .
sudo install -Dm644 99-g15-led.rules /etc/udev/rules.d/99-g15-led.rules
sudo udevadm control --reload && sudo udevadm trigger
```

### Fan/power support (optional, needs root)

Fan boost and power modes go through the WMAX ACPI method via the `acpi_call`
kernel module:

```sh
sudo pacman -S acpi_call        # or acpi_call-dkms / acpi_call-lts
echo acpi_call | sudo tee /etc/modules-load.d/acpi_call.conf
sudo modprobe acpi_call
```

## Protecting the controller

**Required on every install.** Anything that writes the SMBIOS keyboard-backlight
LED device will wedge the USB controller until a full power drain:

```sh
# stop systemd restoring a saved brightness into it at every boot
sudo systemctl mask 'systemd-backlight@leds:dell::kbd_backlight.service'
```

Then remove/disable anything else that touches `*::kbd_backlight` via
`brightnessctl` or sysfs. On Omarchy that means:

- in `~/.config/hypr/hypridle.conf`: delete the listener that runs
  `brightnessctl -sd '*::kbd_backlight' set 0` on idle
- never bind keys to `omarchy-brightness-keyboard` — and note that
  `omarchy-system-lock` calls `omarchy-brightness-keyboard off` 3 s after
  locking, so **every idle/manual lock wedges the controller**. Shadow it with
  an update-proof shim: put a `omarchy-brightness-keyboard` script in
  `~/.local/bin` that maps `off`→`g15 led off`, `restore`→`g15 restore`, and
  everything else→`g15 led brightness cycle`, then make `~/.local/bin` precede
  omarchy's bin dir by appending `export PATH=$HOME/.local/bin:$PATH` to
  `~/.config/uwsm/env` (user config — survives omarchy updates)
- `/usr/lib/systemd/system-sleep/keyboard-backlight` writes it before every
  hibernate — delete it or add `[[ -e /sys/class/leds/dell::kbd_backlight ]] && exit 0`
  near the top

If the backlight stops responding and survives reboots: shut down, unplug AC,
disconnect the battery (or hold the power button 30 s with both removed), boot.
The `g15` CLI detects the wedged state and tells you.

The full Omarchy setup — including Fn-key/G-Mode key bindings and an hwdb
remap for the internal keyboard — is documented in [omarchy.md](omarchy.md).

## Usage

```
g15 led RRGGBB [brightness 0-100]   static color
g15 led pulse RRGGBB [speed]        breathe (speed 1-10, default 5)
g15 led morph RRGGBB RRGGBB [speed] morph between two colors
g15 led cycle [speed]               morph through the color spectrum
g15 led rainbow [speed]             moving rainbow across the 4 zones
g15 led brightness <0-100|cycle>    brightness; cycle = off -> 50% -> 100%
g15 led off | on

sudo g15 power                      show power mode
sudo g15 power gmode                balanced|performance|quiet|battery|gmode
sudo g15 power toggle               gmode <-> previous mode (for the G key)
sudo g15 fan boost 0-100
sudo g15 info                       model, firmware, temps, fan rpm

g15 tui                             interactive panel (root for fan/power tab)
g15 waybar                          JSON for a waybar custom module
g15 restore                         re-apply saved LED state (run at login)
```

Settings persist to `~/.config/g15/state`; `g15 restore` replays them (the
firmware loses its animation on every USB re-enumeration, so run it at login).
Sensors come root-free from the `alienware_wmi`/`dell_smm` hwmon.

## Desktop integration

### Keybind: the Fn keyboard-backlight key

Fn+F5 on the G15 emits kernel `KEY_F18` (scancode 0x69) — Dell handles it in
AWCC software on Windows, so there is no kbd-illumination keysym. XKB presents
it as **`XF86Launch9`**. Hyprland:

```
bind = , XF86Launch9, exec, g15 led brightness cycle
```

sway is identical; for X11 use `xbindkeys` with `XF86Launch9`.

### Keybind: the G-Mode key (Fn+F9)

Fn+F9 emits kernel `KEY_PERFORMANCE` (keycode 701, scancode 0x68). That's above
XKB's 8-bit keysym range, so there is no keysym — bind it by raw keycode
(evdev 701 + 8 = 709). Hyprland:

```
bind = , code:709, exec, sh -c 'notify-send "Power mode" "$(sudo -n g15 power toggle 2>&1)"'
```

`g15 power toggle` switches to gmode and back to whatever mode you were in
before, printing the new mode. It needs root without a password prompt:

```sh
echo "$USER ALL=(root) NOPASSWD: /usr/bin/g15 power toggle" | \
  sudo tee /etc/sudoers.d/g15-power-toggle && sudo chmod 440 /etc/sudoers.d/g15-power-toggle
```

### Autostart (restore LED state at login)

Hyprland: `exec-once = g15 restore` (Omarchy lua config:
`o.launch_on_start("g15 restore")` in `~/.config/hypr/autostart.lua`).

### Waybar module

```jsonc
"custom/g15": {
  "exec": "g15 waybar",
  "return-type": "json",
  "interval": 5,
  "on-click": "alacritty --class=g15-tui -e sudo g15 tui"
}
```

Shows CPU/GPU temps in the bar, fans + power mode on hover. On Omarchy, use
`"on-click": "omarchy-launch-or-focus-tui g15-tui"` with a `g15-tui` wrapper
script on PATH (`exec sudo g15 tui`) to get the standard floating TUI window,
and float it in `~/.config/hypr/apps.lua`:

```lua
o.window("org.omarchy.g15-tui", { tag = "+floating-window" })
```

### Example profile binds

```
bind = SUPER, F1, exec, g15 led 00ff88 && sudo g15 power quiet
bind = SUPER, F2, exec, g15 led rainbow 7 && sudo g15 power gmode
```

(`sudo` in binds needs a NOPASSWD sudoers entry for `/usr/bin/g15`, or route
power changes through the TUI instead.)

## Notes

- Only tested on a G15 5520 (Intel). The WMAX fan/power codes come from
  [dell-g-series-controller](https://github.com/cemkaya-mpi/Dell-G-Series-Controller);
  AMD models (AMW3 method) are attempted as a fallback but untested.
- The TUI's screen color picker uses `hyprpicker` (optional).
- MIT licensed. Protocol notes in [protocol.md](protocol.md) are the
  interesting part if you're porting this to another OS or tool.
