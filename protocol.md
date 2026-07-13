# G15 5520 keyboard LED protocol (decoded from AWCC captures)

**Status: WORKING (verified 2026-07-09).** The four stacked problems, each of
which masked the others:
1. Transport: control pipe only (HIDIOCSOUTPUT/HIDIOCGINPUT); interrupt EPs dead.
2. Color zones are 0x10–0x13, not 0–3 (dell-g-series-controller's bug).
3. Command 0x26 is DIMMING, inverted: 0 = full brightness, 100 = off.
4. Any write to /sys/class/leds/dell::kbd_backlight (SMBIOS→EC) wedges the
   controller until full power removal. Omarchy wrote it at boot (systemd-backlight,
   now masked), on idle (hypridle listener, now commented out), and on brightness
   keys (omarchy-brightness-keyboard — never call it on this machine).

Device: `187c:0550`, HID, no report IDs. 33-byte reports, first byte always `0x03`.

## Transport — the critical part

The firmware **only responds on the USB control pipe**: SET_REPORT (output, id 0)
to send, GET_REPORT (input, 33 bytes) to read the ack. The interrupt endpoints
exist but are dead — plain hidraw `write()`/`read()` sends into the void and never
gets a response. **This is why dell-g-series-controller fails on this machine.**

On Linux use the hidraw ioctls (kernel ≥5.11): `HIDIOCSOUTPUT` to send,
`HIDIOCGINPUT` to read. Buffer = 1 report-number byte (0) + 33 data bytes.
In Rust: hidapi's `send_output_report`... check it uses control pipe, else nix::ioctl.
Read the ack after every command — AWCC always does.

Two other gotchas vs dell-g-series-controller:
- Color zones are `0x10 0x11 0x12 0x13` (not 0–3). Dimming addresses all 20 zones `0x00`–`0x13`.
- Never `device.reset()` — the controller can wedge until a full EC reset
  (power button 30 s with AC + battery out).

## Commands (33 bytes, zero-padded, all start `0x03`)

```
03 20 02                                  # get platform -> ack 03 20 02 0e 07 14 (0x14 = 20 zones)
03 26 <dim> 00 14 00 01 .. 13             # DIMMING, inverted: 0x00 full, 0x32 half, 0x64 off
                                          # (fn capture cycle 00/64/32 = high/off/low)
03 21 00 <sub> <anim:u16>                 # animation ctl: 01 start-new, 02 finish-save,
                                          #   03 finish-play, 04 remove, 06 set-default
03 23 01 00 <count:u16... actually 00 <n>> <zones...>   # start series (loop=01)
03 24 <action> [<action>] [<action>]      # add up to 3 chained 8-byte actions
```

Action = 8 bytes: `<effect:u8> <duration:u16be> <tempo:u16be> <R> <G> <B>`
(effect: 00 static, 01 pulse, 02 morph). More than 3 actions → send more `03 24` packets.

## Captured sequences (what AWCC actually sends)

Apply color/effect NOW (AWCC uses this for everything, incl. previews —
it never saved to a persistent slot in the whole session):

```
03 21 00 01 ff ff                  # start new RUNNING animation (id 0xffff)
03 23 01 00 04 10 11 12 13         # series over the 4 keyboard zones
03 24 00 07 d0 00 fa RR GG BB      # static color (dur 2000, tempo 250)
03 21 00 03 00 ff                  # finish-play RUNNING (id 0x00ff)
```

Effect variants observed in the `03 24` slot:
- pulse:    `01 07d0 0064 RGB`
- morph 2-color: `02 05dc 0064 RGB1` + `02 05dc 0064 RGB2` chained
- spectrum: seven `02 0282 000f RGB` actions across 3 packets
- wave: per-zone series (`03 23 01 00 01 <zone>`) each with the same morph
  chain (`02 01ac 000f RGB` ×7) rotated by zone

Brightness change (Fn key or slider): `03 20 02` then `03 26 <dim> 00 14 <20 zones>`.

Persistent (survives reboot) — from elc protocol, not exercised by AWCC in capture:
same as above but anim id `0x0061` (DEFAULT_POST_BOOT), `02` finish-save instead of
`03` finish-play, then `03 21 00 06 00 61` set-default.

## Wedged-controller signature

Command parser stays alive — GET_VERSION (fw 1.1.12), GET_STATUS, GET_PLATFORM all
answer correctly — but LEDs never change and **acks are verbatim echoes of the full
command** (e.g. dim ack = `03 26 64 00 14 00 01 02...`). A healthy controller
returns short status acks (`03 26 64 00 00...`, `03 23 00`, `03 24 00`) as seen in
the working Windows fn-trigger capture. Echo-style acks = wedged.

Recovery: `USBDEVFS_RESET` ioctl on /dev/bus/usb/BBB/DDD flipped the acks back to
healthy style (firmware restart) — **once**. On the 2026-07-10 wedge it did not:
repeated USBDEVFS_RESET, root-hub port power cycle (`usb3-port7/disable`), and
re-enumeration all left echo-mode acks intact, so the state lives on the EC's
standby rail, not the USB function. Hard fallback: EC reset — power btn 30 s with AC
and battery disconnected; the wedge survives warm reboots and even S5 (5 V standby).
Wedge trigger is still unconfirmed: candidates are the enumeration port reset at
Linux boot, OpenRGB probing it, or the old device.reset() experiments.

**Prime suspect for the wedge trigger (2026-07-09): writes to
`/sys/class/leds/dell::kbd_backlight`** (dell-laptop → SMBIOS → EC). Evidence:
backlight survives initramfs/LUKS (USB enumerated, no SMBIOS activity) and dies
right when systemd-backlight restores the saved value (t+25.8 s, the "login"
moment); controller found wedged on a boot where no user apps ran; Windows never
wedges and AWCC never touches SMBIOS (Fn+F5 is AWCC software sending USB dim —
the fn-trigger capture proves it). Writers on omarchy: systemd-backlight restore
at boot, hypridle idle listener (330 s), omarchy-brightness-keyboard keybind.
All must be disabled: hypridle listener commented out in ~/.config/hypr/hypridle.conf,
systemd unit masked (`systemctl mask 'systemd-backlight@leds:dell::kbd_backlight.service'`),
never run brightnessctl against `*::kbd_backlight`. Fourth writer found 2026-07-13:
`/usr/lib/systemd/system-sleep/keyboard-backlight` (omarchy's ASUS hibernate fix)
runs `brightnessctl -d *kbd_backlight* set 0` pre-hibernate — was non-executable
here so it never fired, but it's now guarded with an early exit on dell hardware.

**2026-07-10 wedge — SMBIOS writers ruled out for this instance.** Wedged again
with the systemd unit still masked, hypridle listener still disabled, LED trigger
`[none]`, and zero kbd_backlight/brightnessctl journal entries. New activity that
day: WMAX power-mode toggling (`0x15` mode set + `0x25` G-mode flag, ~6 switches
balanced<->gmode) and physical Fn+F9 (KEY_PERFORMANCE) presses. Sleep/hibernate
ruled out for this instance (journal shows zero suspend/hibernate cycles since
2026-07-09). A wedge watchdog now probes the controller every 2 min
(`~/.local/bin/g15-wedge-watch`, systemd user timer): logs transitions to
`~/.local/state/g15-wedge-watch.log` and snapshots the journal + g15 state on
OK→WEDGED, to timestamp the next wedge precisely.

**2026-07-13 — TRIGGER CAUGHT: the idle-lock path.** The watchdog nailed it on
its first day. Timeline: probe OK 17:48:35 → hypridle idle chain fires
(`omarchy-launch-screensaver` 150 s, `omarchy-system-lock` 152 s) at 17:50:00 →
`omarchy-system-lock` runs `omarchy-brightness-keyboard off` 3 s after hyprlock
starts, i.e. `brightnessctl -sd dell::kbd_backlight set 0` — and the kernel logs
a `dell-privacy` WMI event at exactly 17:50:03 (the SMBIOS call) → probe WEDGED
17:50:35. This 5th writer hides inside the lock script, so every idle-lock (or
manual lock) wedged the controller — which also explains the 2026-07-10 wedge
(machine idle-locked while unattended) and likely exonerates the WMAX G-mode
path. A Hyprland/compositor crash 2 min later (17:52:47, after an NVIDIA "HDMI
FRL link training failed") was unrelated to the wedge. Fix: guard at the top of
`~/.local/share/omarchy/bin/omarchy-brightness-keyboard` reroutes off/restore/
cycle to `g15` (USB path) when dell::kbd_backlight exists — note omarchy updates
may overwrite it; the watchdog will catch a regression.

Working reference: `led-test.py` (`python3 led-test.py RR GG BB [dim]`, no root
needed with the uaccess udev rule / existing ACL on /dev/hidraw0).
