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
  g15 led pulse RRGGBB [speed]        pulse effect (speed 1-10, default 5)
  g15 led morph RRGGBB RRGGBB [speed] morph between two colors
  g15 led cycle [speed]               morph through the color spectrum
  g15 led rainbow [speed]             moving rainbow across the 4 zones
  g15 led brightness <0-100|cycle>    brightness; cycle = off -> 50% -> 100%
  g15 led effect <name>               re-apply an effect with its saved colors
  g15 led colors RRGGBB[,RRGGBB...]   set the current effect's color list
  g15 led speed <1-10>                set the current effect's speed
  g15 led off | on

Power/fans (root + acpi_call module):
  g15 power                           show current mode
  g15 power balanced|performance|quiet|battery|gmode
  g15 power toggle                    gmode <-> previous mode (G key)
  g15 fan                             show fan boost + rpm
  g15 fan boost <0-100>               set boost on both fans
  g15 info                            model, fw, mode, temps, rpm

Desktop integration:
  g15 tui                             interactive two-tab control panel
  g15 waybar                          JSON stats for a waybar custom module
  g15 status                          JSON stats for the omarchy bar plugin
  g15 restore                         re-apply saved LED state (autostart)";

/// Color list an effect was last configured with (TUI or CLI), else spectrum.
fn saved_colors(effect: &str) -> Vec<(u8, u8, u8)> {
    state::load()
        .get(&format!("colors_{effect}"))
        .map(|l| l.split(',').filter_map(|h| parse_rgb(h).ok()).collect::<Vec<_>>())
        .filter(|v| v.len() >= 2)
        .unwrap_or_else(|| led::SPECTRUM.to_vec())
}

/// Apply `effect` with the speed and colors last saved for it. Shared by
/// `g15 restore` (autostart) and `g15 led effect` (bar plugin).
fn apply_saved_effect(led: &led::Led, effect: &str) -> std::io::Result<()> {
    let saved = state::load();
    let speed = parse_speed(saved.get("speed").map(String::as_str)).unwrap_or(5);
    let colors = saved_colors(effect); // >=2 entries or spectrum
    let one = saved
        .get(&format!("colors_{effect}"))
        .and_then(|l| parse_rgb(l.split(',').next().unwrap_or("")).ok())
        .unwrap_or((255, 255, 255));
    match effect {
        "pulse" => led.pulse(one.0, one.1, one.2, speed),
        "morph" => led.morph(colors[0], colors[1], speed),
        "cycle" => led.cycle(&colors, speed),
        "rainbow" => led.rainbow(&colors, speed),
        _ => led.color(one.0, one.1, one.2),
    }
}

/// The effect the last `g15 led` / TUI change left in the state file.
fn current_effect() -> String {
    state::load()
        .get("effect")
        .filter(|e| led::EFFECTS.contains(&e.as_str()))
        .cloned()
        .unwrap_or_else(|| "static".into())
}

fn parse_speed(s: Option<&str>) -> Result<u8, String> {
    match s {
        None => Ok(5),
        Some(v) => match v.parse::<u8>() {
            Ok(n @ 1..=10) => Ok(n),
            _ => Err(format!("speed must be 1-10, got '{v}'")),
        },
    }
}

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
                    let p: u8 = match arg(2) {
                        // off -> 50% -> 100% -> off (bound to the Fn kbd key)
                        Some("cycle") => {
                            let cur: u8 = state::load()
                                .get("brightness")
                                .and_then(|b| b.parse().ok())
                                .unwrap_or(100);
                            if cur < 25 {
                                50
                            } else if cur < 75 {
                                100
                            } else {
                                0
                            }
                        }
                        other => other
                            .and_then(|s| s.parse().ok())
                            .ok_or("usage: g15 led brightness <0-100|cycle>")?,
                    };
                    led.brightness(p)
                        .and_then(|()| state::set("brightness", &p.to_string()))
                        .map_err(|e| e.to_string())?;
                    // best-effort OSD popup (display-only omarchy helper)
                    let _ = std::process::Command::new("omarchy-swayosd-kbd-brightness")
                        .arg(p.to_string())
                        .spawn();
                    Ok(())
                }
                Some("pulse") => {
                    let hex = arg(2).ok_or("usage: g15 led pulse RRGGBB [speed]")?;
                    let (r, g, b) = parse_rgb(hex)?;
                    let speed = parse_speed(arg(3))?;
                    led.pulse(r, g, b, speed).and_then(|()| {
                        state::set("effect", "pulse")?;
                        state::set("speed", &speed.to_string())?;
                        state::set("colors_pulse", hex.trim_start_matches('#'))
                    })
                }
                Some("morph") => {
                    let h1 = arg(2).ok_or("usage: g15 led morph RRGGBB RRGGBB [speed]")?;
                    let h2 = arg(3).ok_or("usage: g15 led morph RRGGBB RRGGBB [speed]")?;
                    let (c1, c2) = (parse_rgb(h1)?, parse_rgb(h2)?);
                    let speed = parse_speed(arg(4))?;
                    led.morph(c1, c2, speed).and_then(|()| {
                        state::set("effect", "morph")?;
                        state::set("speed", &speed.to_string())?;
                        state::set(
                            "colors_morph",
                            &format!(
                                "{},{}",
                                h1.trim_start_matches('#'),
                                h2.trim_start_matches('#')
                            ),
                        )
                    })
                }
                Some("cycle") => {
                    let speed = parse_speed(arg(2))?;
                    let colors = saved_colors("cycle");
                    led.cycle(&colors, speed).and_then(|()| {
                        state::set("effect", "cycle")?;
                        state::set("speed", &speed.to_string())
                    })
                }
                Some("rainbow") => {
                    let speed = parse_speed(arg(2))?;
                    let colors = saved_colors("rainbow");
                    led.rainbow(&colors, speed).and_then(|()| {
                        state::set("effect", "rainbow")?;
                        state::set("speed", &speed.to_string())
                    })
                }
                Some("effect") => {
                    let name = arg(2)
                        .ok_or("usage: g15 led effect <static|pulse|morph|cycle|rainbow>")?;
                    if !led::EFFECTS.contains(&name) {
                        return Err(format!("unknown effect '{name}'"));
                    }
                    apply_saved_effect(&led, name)
                        .and_then(|()| state::set("effect", name))
                }
                Some("colors") => {
                    // Edits the current effect's list — the same `colors_<effect>`
                    // entries the TUI's swatch row writes.
                    let list = arg(2).ok_or("usage: g15 led colors RRGGBB[,RRGGBB...]")?;
                    let colors: Vec<&str> = list.split(',').map(str::trim).collect();
                    for c in &colors {
                        parse_rgb(c)?;
                    }
                    let effect = current_effect();
                    let (min, max) = led::color_limits(&effect);
                    if colors.len() < min || colors.len() > max {
                        return Err(format!(
                            "{effect} takes {}",
                            if min == max { format!("exactly {min} color(s)") } else { format!("{min}-{max} colors") }
                        ));
                    }
                    let normalized: Vec<String> = colors
                        .iter()
                        .map(|c| c.trim_start_matches('#').to_ascii_lowercase())
                        .collect();
                    state::set(&format!("colors_{effect}"), &normalized.join(","))
                        .and_then(|()| apply_saved_effect(&led, &effect))
                }
                Some("speed") => {
                    let speed = parse_speed(arg(2))?;
                    state::set("speed", &speed.to_string())
                        .and_then(|()| apply_saved_effect(&led, &current_effect()))
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
                        state::set("colors_static", color.trim_start_matches('#'))
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
            Some("toggle") => {
                let cur = wmax::get_power_mode().map_err(|e| e.to_string())?;
                let name = if cur == 0xAB {
                    // leaving gmode: back to whatever was set before it
                    state::load().get("power_prev").cloned().unwrap_or("balanced".into())
                } else {
                    let prev = wmax::POWER_MODES
                        .iter()
                        .find(|(_, v)| *v as u32 == cur)
                        .map(|(n, _)| *n)
                        .unwrap_or("balanced");
                    state::set("power_prev", prev).map_err(|e| e.to_string())?;
                    "gmode".into()
                };
                let (_, mode) = wmax::POWER_MODES.iter().find(|(n, _)| *n == name).unwrap();
                wmax::set_power_mode(*mode).map_err(|e| e.to_string())?;
                state::set("power", &name).map_err(|e| e.to_string())?;
                println!("{name}");
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
                    "{{\"text\": \"󰍛 {}°  󰢮 {}°\", \"tooltip\": \"CPU {}°C — fan {} rpm\\nGPU {}°C — fan {} rpm\\npower: {}\"}}",
                    s.cpu, s.gpu, s.cpu, s.fan1, s.gpu, s.fan2, power
                ),
                Err(_) => println!("{{\"text\": \"󰌌\", \"tooltip\": \"sensors unavailable\"}}"),
            }
            Ok(())
        }
        Some("status") => {
            // Structured sibling of `waybar`, for the omarchy bar plugin's panel.
            // Same guarantee: hwmon + state file only, never the USB device.
            let saved = state::load();
            let power = saved
                .get("power")
                .filter(|p| wmax::POWER_MODES.iter().any(|(n, _)| n == p))
                .map(String::as_str)
                .unwrap_or("unknown");
            let effect = saved.get("effect").map(String::as_str).unwrap_or("static");
            let brightness: u8 = saved
                .get("brightness")
                .and_then(|b| b.parse().ok())
                .unwrap_or(100);
            // Reading the real boost needs root (WMAX); report what was last set.
            let boost: u8 = saved.get("boost").and_then(|b| b.parse().ok()).unwrap_or(0);
            let speed: u8 = parse_speed(saved.get("speed").map(String::as_str)).unwrap_or(5);
            let (min, max) = led::color_limits(effect);
            let colors: Vec<String> = saved
                .get(&format!("colors_{effect}"))
                .map(|l| {
                    l.split(',')
                        .map(|c| c.trim().trim_start_matches('#').to_ascii_lowercase())
                        .filter(|c| parse_rgb(c).is_ok())
                        .take(max)
                        .collect()
                })
                .filter(|v: &Vec<String>| v.len() >= min)
                .unwrap_or_else(|| vec!["ffffff".into(); min]);
            let color = colors[0].clone();
            let colors_json = colors
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(",");
            let s = hwmon::read().ok();
            let ok = s.is_some();
            let s = s.unwrap_or(hwmon::Stats { cpu: 0, gpu: 0, fan1: 0, fan2: 0 });
            println!(
                "{{\"sensors\":{ok},\"cpu\":{},\"gpu\":{},\"fan1\":{},\"fan2\":{},\
\"power\":\"{power}\",\"boost\":{boost},\"effect\":\"{effect}\",\"brightness\":{brightness},\
\"speed\":{speed},\"color\":\"{color}\",\"colors\":[{colors_json}],\
\"minColors\":{min},\"maxColors\":{max}}}",
                s.cpu, s.gpu, s.fan1, s.fan2
            );
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
            let effect = saved.get("effect").map(String::as_str).unwrap_or("static");
            apply_saved_effect(&led, effect).map_err(|e| e.to_string())
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
