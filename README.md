# g15

Keyboard backlight, fan, and power-mode control for the Dell G15 5520 on Linux.
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
   battery removed). Mask `systemd-backlight@leds:dell::kbd_backlight.service`
   and remove any idle/keybind hooks that touch that LED device.

## Install

```sh
cargo install --path .
```

LED control is unprivileged if your user can open the hidraw device (an
OpenRGB-style udev rule for `187c:0550` with `TAG+="uaccess"` does it).
Fan/power control needs root plus the `acpi_call` kernel module.

## Usage

```
g15 led RRGGBB [brightness]      static color
g15 led pulse RRGGBB             pulse effect
g15 led morph RRGGBB RRGGBB      morph between two colors
g15 led brightness 0-100
g15 led off | on

sudo g15 power                   show power mode
sudo g15 power gmode             balanced|performance|quiet|battery|gmode
sudo g15 fan boost 0-100
sudo g15 info                    model, firmware, temps, fan rpm

g15 tui                          two-tab interactive panel (ratatui)
g15 waybar                       JSON for a waybar custom module
g15 restore                      re-apply saved LED state (run at login)
```

Sensors for `waybar`/`tui` are read root-free from the `alienware_wmi`/`dell_smm`
hwmon. Settings persist to `~/.config/g15/state`.

## Waybar / Hyprland (Omarchy) integration

```jsonc
"custom/g15": {
  "exec": "~/.cargo/bin/g15 waybar",
  "return-type": "json",
  "interval": 5,
  "on-click": "omarchy-launch-or-focus-tui g15-tui"
}
```

where `g15-tui` is a one-line wrapper: `exec sudo ~/.cargo/bin/g15 tui`.
Autostart: `o.launch_on_start("~/.cargo/bin/g15 restore")`.

The Fn keyboard-backlight key (Fn+F5) emits kernel `KEY_F18` (scancode 0x69),
which XKB presents as `XF86Launch9` — Dell handles this key in AWCC software on
Windows, so no kbd-illumination keysym exists. Bind it to the cycle command:

```
bind = , XF86Launch9, exec, g15 led brightness cycle
```

Do NOT keep omarchy's default `XF86KbdBrightness*`/`XF86KbdLightOnOff` binds to
`omarchy-brightness-keyboard` — that writes the SMBIOS LED device and wedges
the controller (gotcha 4 above).

Only tested on a G15 5520 (Intel). The WMAX fan/power codes come from
[dell-g-series-controller](https://github.com/cemkaya-mpi/Dell-G-Series-Controller);
AMD models (AMW3 method) are attempted as a fallback but untested.
