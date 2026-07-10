Here's a concrete implementation plan, built directly on what we've already reverse-engineered from dell-g-series-controller plus the lessons learned from this whole debugging saga.
What We're Reusing From the Python App
Fan/Power control is solid and fully reusable as a protocol reference — it doesn't touch USB at all, just the acpi_call kernel module via /proc/acpi/call, invoking a WMI method:
\_SB.AMWW.WMAX 0 {op} {{{sub}, {arg}, {arg2}, 0x00}}     <- Intel models
\_SB.AMW3.WMAX 0 {op} {{{sub}, {arg}, {arg2}, 0x00}}     <- AMD models
Known op/sub codes (from main.py:52-77):
Function
Get laptop model
Get/set power mode
Toggle/get G-Mode
Fan1 boost set/get
Fan2 boost set/get
Fan1/2 RPM
CPU/GPU temp
Power mode values: Balanced=0xa0, Performance=0xa1, Quiet=0xa3, FullSpeed=0xa4, BatterySaver=0xa5, GMode=0xab. Your model (G15 5520) drops FullSpeed (see patch.py:4-5) — laptop model ID 0x12c0 on the Intel WMAX path identifies it (main.py:122).
Keyboard LED is NOT reusable yet — pending your Wireshark capture, since we've proven the existing command sequence doesn't work on your firmware.
Security Issue Worth Fixing (found while reading the code)
The Python app builds a shell string via Python .format() with slider-controlled values, then executes it in a pkexec-spawned root bash shell (main.py:108, acpi_call()). This is a real shell-injection risk — any value that escapes the expected hex-format assumption gets executed as root. The Rust version should avoid a persistent root shell entirely.
Proposed Architecture
┌─────────────────────────┐        unix socket        ┌──────────────────────────┐
│  g15ctl-tray (user)     │ ───────────────────────── │  g15ctld (root daemon)   │
│  - system tray icon     │   strict typed protocol    │  - opens /proc/acpi/call │
│  - global hotkeys       │   (no shell strings ever)   │  - opens /dev/hidraw or  │
│  - config file          │                             │    libusb HID device    │
│  - calls daemon via IPC │                             │  - whitelisted commands  │
└─────────────────────────┘                             └──────────────────────────┘
- g15ctld — small root-owned daemon (systemd service, Type=simple, started via a .service unit, socket at /run/g15ctld.sock with mode 0660 group g15ctl). Exposes a tiny fixed set of operations as a typed enum (e.g., SetPowerMode(Mode), SetFanBoost(Fan, u8), GetTelemetry, SetKeyboardColor(u8,u8,u8)) — never raw strings. This avoids repeated pkexec prompts (the daemon is always root, started once at boot) and eliminates the injection risk since there's no string formatting into a shell at all — just direct byte writes to /proc/acpi/call and direct HID reports via hidapi/rusb.
- g15ctl-tray — unprivileged user process: tray icon (via tray-icon or ksni crate), reads/writes a TOML config in ~/.config/g15ctl/, registers global hotkeys (via global-hotkey crate, works under Wayland/Hyprland with some caveats — may need to lean on Hyprland's own bind→exec mechanism instead, invoking g15ctl-tray --action set-color-profile-2 from a Hyprland keybind rather than a true global-hotkey listener, which is more reliable on Wayland).
Phased Plan
Phase 0 — Protocol capture (you're already doing this)
Capture AWCC's real HID traffic via Wireshark+USBPcap on Windows, extract the working SET_REPORT/feature-report byte sequences for turning the backlight on with a static color.
Phase 1 — g15ctld core (fan/power), no LED yet
- Rust crate, tokio + tokio-uds (or interprocess crate) for the socket.
- Port the WMAX call table above verbatim as a Rust enum + match statement writing to /proc/acpi/call directly (no shell).
- Add laptop-model detection + the per-model quirks from patch.py (drop FullSpeed for 5520, etc.) as a static table keyed by the detected model ID.
- Systemd unit + udev/tmpfiles rule for the socket permissions, gated to a new g15ctl group.
- CLI test client to validate fan/power control works before building UI.
Phase 2 — Modprobe/module concerns baked in properly
- Ship a modules-load.d drop-in for acpi_call as part of install docs/packaging (equivalent to what we manually set up at /etc/modules-load.d/acpi_call.conf).
- Daemon should check the module is loaded at startup and fail with a clear error/log message rather than silently misbehaving.
Phase 3 — Keyboard LED module
- Implement using hidapi crate against /dev/hidraw* matching 187c:0550/0551 (ship the same udev rule OpenRGB uses: MODE="0660", TAG+="uaccess", so it doesn't even need to go through the root daemon — this part can stay in the user-level tray process, unlike fan/power).
- Implement the exact byte sequence captured from Windows, parameterized by RGB + zone list.
- Critical lesson baked in from this whole debugging saga: never call a full USB device reset as part of normal operation. Only use hidapi's feature report read/write; no libusb-level reset() calls at all unless recovering from an actual error state.
- Add a simple in-process rate limiter (e.g., debounce/coalesce color-apply requests to at most 1 per 500ms) so rapid UI interaction can never hammer the device the way the Python app did.
Phase 4 — Tray UI
- Menu: power mode selector, fan boost sliders (or presets), keyboard color presets, on/off toggle.
- Config persisted to ~/.config/g15ctl/config.toml (profiles: color + power-mode combos).
- Expose actions as CLI subcommands too (g15ctl-tray set-profile gaming) so they can be bound directly to Hyprland keybinds via bind = ..., exec, g15ctl-tray set-profile gaming rather than relying on in-app global hotkeys.
Phase 5 — Polish
- Startup autostart entry (~/.config/omarchy/ hook or plain ~/.config/autostart/, or wired into your Hyprland autostart.conf/.lua — matching the pattern we already used for hypr/apps.lua).
- Optional: expose fan RPM/temp as a Waybar custom module reading from the daemon's socket, since you're already on Omarchy/Hyprland/Waybar.
What I need from you to keep moving
Once you have the Wireshark capture, send me the relevant HID report bytes (or the .pcapng) and I'll help decode the exact working sequence — that unblocks Phase 3, which is really the only genuinely new reverse-engineering work left. Phases 1-2 (fan/power) can start immediately since we already have everything needed from the Python source.
Want me to start scaffolding the Rust project structure (Cargo workspace with g15ctld + g15ctl-tray crates) now, even before the LED protocol is nailed down?
