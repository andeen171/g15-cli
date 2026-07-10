mod hwmon;
mod led;
mod state;
mod tui;
mod wmax;

use std::process::exit;

const USAGE: &str = "\
g15 — Dell G15 5520 keyboard backlight + fan/power control

LED (no root needed):
  g15 led RRGGBB [brightness 0-100]   static color
  g15 led pulse RRGGBB                pulse effect
  g15 led morph RRGGBB RRGGBB         morph between two colors
  g15 led brightness <0-100>          brightness only
  g15 led off | on

Power/fans (root + acpi_call module):
  g15 power                           show current mode
  g15 power balanced|performance|quiet|battery|gmode
  g15 fan                             show fan boost + rpm
  g15 fan boost <0-100>               set boost on both fans
  g15 info                            model, fw, mode, temps, rpm

Desktop integration:
  g15 tui                             interactive two-tab control panel
  g15 waybar                          JSON stats for a waybar custom module
  g15 restore                         re-apply saved LED state (autostart)";

fn parse_rgb(s: &str) -> Result<(u8, u8, u8), String> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("bad color '{s}', expected RRGGBB hex"));
    }
    let v = u32::from_str_radix(s, 16).unwrap();
    Ok(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).map(String::as_str);

    match arg(0) {
        Some("led") => {
            let led = led::Led::open().map_err(|e| e.to_string())?;
            match arg(1) {
                Some("off") => led.brightness(0),
                Some("on") => led.brightness(100),
                Some("brightness") => {
                    let p: u8 = arg(2)
                        .and_then(|s| s.parse().ok())
                        .ok_or("usage: g15 led brightness <0-100>")?;
                    led.brightness(p)
                        .and_then(|()| state::set("brightness", &p.to_string()))
                }
                Some("pulse") => {
                    let hex = arg(2).ok_or("usage: g15 led pulse RRGGBB")?;
                    let (r, g, b) = parse_rgb(hex)?;
                    led.pulse(r, g, b).and_then(|()| {
                        state::set("effect", "pulse")?;
                        state::set("color", hex.trim_start_matches('#'))
                    })
                }
                Some("morph") => {
                    let h1 = arg(2).ok_or("usage: g15 led morph RRGGBB RRGGBB")?;
                    let h2 = arg(3).ok_or("usage: g15 led morph RRGGBB RRGGBB")?;
                    let (c1, c2) = (parse_rgb(h1)?, parse_rgb(h2)?);
                    led.morph(c1, c2).and_then(|()| {
                        state::set("effect", "morph")?;
                        state::set("color", h1.trim_start_matches('#'))?;
                        state::set("color2", h2.trim_start_matches('#'))
                    })
                }
                Some(color) => {
                    let (r, g, b) = parse_rgb(color)?;
                    if let Some(p) = arg(2) {
                        let p: u8 = p.parse().map_err(|_| "brightness must be 0-100")?;
                        led.brightness(p).map_err(|e| e.to_string())?;
                        state::set("brightness", &p.to_string()).map_err(|e| e.to_string())?;
                    }
                    led.color(r, g, b).and_then(|()| {
                        state::set("effect", "static")?;
                        state::set("color", color.trim_start_matches('#'))
                    })
                }
                None => Err(std::io::Error::other("missing led argument, see g15 --help")),
            }
            .map_err(|e| e.to_string())
        }
        Some("power") => match arg(1) {
            None => {
                let mode = wmax::get_power_mode().map_err(|e| e.to_string())?;
                let name = wmax::POWER_MODES
                    .iter()
                    .find(|(_, v)| *v as u32 == mode)
                    .map(|(n, _)| *n)
                    .unwrap_or("unknown");
                println!("{name} ({mode:#x})");
                Ok(())
            }
            Some(name) => {
                let (_, mode) = wmax::POWER_MODES
                    .iter()
                    .find(|(n, _)| *n == name)
                    .ok_or(format!("unknown mode '{name}'"))?;
                wmax::set_power_mode(*mode).map_err(|e| e.to_string())?;
                state::set("power", name).map_err(|e| e.to_string())
            }
        },
        Some("fan") => match (arg(1), arg(2)) {
            (Some("boost"), Some(v)) => {
                let boost: u8 = v.parse().map_err(|_| "boost must be 0-100")?;
                for fan in 0..2 {
                    wmax::set_fan_boost(fan, boost.min(100)).map_err(|e| e.to_string())?;
                }
                Ok(())
            }
            (None, _) => {
                for fan in 0..2 {
                    let boost = wmax::fan_boost(fan).map_err(|e| e.to_string())?;
                    let rpm = wmax::fan_rpm(fan).map_err(|e| e.to_string())?;
                    println!("fan{}: boost {boost}%  {rpm} rpm", fan + 1);
                }
                Ok(())
            }
            _ => Err("usage: g15 fan [boost <0-100>]".into()),
        },
        Some("tui") => tui::run().map_err(|e| e.to_string()),
        Some("waybar") => {
            let saved = state::load();
            let power = saved.get("power").map(String::as_str).unwrap_or("?");
            match hwmon::read() {
                Ok(s) => println!(
                    "{{\"text\": \"󰌌 {}° 󰈐{}\", \"tooltip\": \"CPU {}°C   GPU {}°C\\nCPU fan {} rpm\\nGPU fan {} rpm\\npower: {}\"}}",
                    s.cpu, s.fan1, s.cpu, s.gpu, s.fan1, s.fan2, power
                ),
                Err(_) => println!("{{\"text\": \"󰌌\", \"tooltip\": \"sensors unavailable\"}}"),
            }
            Ok(())
        }
        Some("restore") => {
            let saved = state::load();
            let led = led::Led::open().map_err(|e| e.to_string())?;
            let brightness = saved
                .get("brightness")
                .and_then(|b| b.parse().ok())
                .unwrap_or(100);
            led.brightness(brightness).map_err(|e| e.to_string())?;
            let color = saved.get("color").map(String::as_str).unwrap_or("ffffff");
            let (r, g, b) = parse_rgb(color)?;
            match saved.get("effect").map(String::as_str) {
                Some("pulse") => led.pulse(r, g, b),
                Some("morph") => {
                    let c2 = parse_rgb(saved.get("color2").map(String::as_str).unwrap_or("0066ff"))?;
                    led.morph((r, g, b), c2)
                }
                _ => led.color(r, g, b),
            }
            .map_err(|e| e.to_string())
        }
        Some("info") => {
            match led::Led::open().and_then(|l| l.fw_version()) {
                Ok(v) => println!("led firmware:  {v}"),
                Err(e) => println!("led firmware:  unavailable ({e})"),
            }
            let model = wmax::laptop_model().map_err(|e| e.to_string())?;
            println!("laptop model:  {model:#x}");
            let mode = wmax::get_power_mode().map_err(|e| e.to_string())?;
            println!("power mode:    {mode:#x}");
            println!("cpu temp:      {}°C", wmax::cpu_temp().map_err(|e| e.to_string())?);
            println!("gpu temp:      {}°C", wmax::gpu_temp().map_err(|e| e.to_string())?);
            for fan in 0..2 {
                println!(
                    "fan{}:          {} rpm",
                    fan + 1,
                    wmax::fan_rpm(fan).map_err(|e| e.to_string())?
                );
            }
            Ok(())
        }
        _ => {
            println!("{USAGE}");
            Ok(())
        }
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("g15: {e}");
        exit(1);
    }
}
