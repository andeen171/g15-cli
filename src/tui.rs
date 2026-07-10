//! Two-tab TUI (Backlight / Fan & Power), omarchy-style. Changes apply live.
//! Run as root (the g15-tui wrapper sudo's) so WMAX works; LED works either way.
//!
//! Each effect keeps its own editable color list. Enter on the Colors row opens
//! an HSV picker (live preview on the keyboard); `p` inside it grabs a color
//! from the screen via hyprpicker.

use crate::{hwmon, led, state, wmax};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, LineGauge, Paragraph};
use ratatui::Frame;
use std::time::Duration;

type Rgb = (u8, u8, u8);

const EFFECTS: [&str; 5] = ["static", "pulse", "morph", "cycle", "rainbow"];
/// (min, max) colors per effect
const COLOR_LIMITS: [(usize, usize); 5] = [(1, 1), (1, 1), (2, 2), (2, 8), (2, 8)];

const PRESETS: [(&str, Rgb); 9] = [
    ("white", (0xFF, 0xFF, 0xFF)),
    ("red", (0xFF, 0x00, 0x00)),
    ("orange", (0xFF, 0x88, 0x00)),
    ("yellow", (0xFF, 0xFF, 0x00)),
    ("green", (0x00, 0xFF, 0x00)),
    ("cyan", (0x00, 0xFF, 0xFF)),
    ("blue", (0x00, 0x66, 0xFF)),
    ("purple", (0x88, 0x00, 0xFF)),
    ("pink", (0xFF, 0x00, 0xAA)),
];

fn hsv_to_rgb(h: u16, s: u8, v: u8) -> Rgb {
    let (h, s, v) = (h as f32, s as f32 / 100.0, v as f32 / 100.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

fn rgb_to_hsv((r, g, b): Rgb) -> (u16, u8, u8) {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h.round() as u16 % 360, (s * 100.0).round() as u8, (max * 100.0).round() as u8)
}

fn hex(c: Rgb) -> String {
    format!("{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn parse_hex(s: &str) -> Option<Rgb> {
    let s = s.trim().trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).ok()?;
    (s.len() == 6).then_some(((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

struct Picker {
    h: u16,
    s: u8,
    v: u8,
    row: usize,                // 0 preset, 1 hue, 2 sat, 3 val
    preset: usize,             // last preset stepped to
    hex_input: Option<String>, // Some(buffer) while typing a hex value
}

impl Picker {
    fn from_rgb(c: Rgb) -> Self {
        let (h, s, v) = rgb_to_hsv(c);
        let preset = PRESETS.iter().position(|&(_, p)| p == c).unwrap_or(0);
        Picker { h, s, v, row: 0, preset, hex_input: None }
    }
}

struct App {
    tab: usize,
    row: usize,
    effect: usize,
    colors: [Vec<Rgb>; 5], // per effect
    sel: usize,            // selected swatch
    speed: u8,
    brightness: u8,
    power: usize,
    boost: u8,
    picker: Option<Picker>,
    status: String,
}

fn default_colors(effect: usize) -> Vec<Rgb> {
    match EFFECTS[effect] {
        "static" => vec![(255, 255, 255)],
        "pulse" => vec![(255, 0, 0)],
        "morph" => vec![(255, 0, 0), (0, 102, 255)],
        _ => led::SPECTRUM.to_vec(),
    }
}

impl App {
    fn from_state() -> Self {
        let s = state::load();
        let effect = s
            .get("effect")
            .and_then(|e| EFFECTS.iter().position(|n| n == e))
            .unwrap_or(0);
        let colors = std::array::from_fn(|i| {
            s.get(&format!("colors_{}", EFFECTS[i]))
                .map(|list| list.split(',').filter_map(parse_hex).collect::<Vec<_>>())
                .filter(|v: &Vec<Rgb>| v.len() >= COLOR_LIMITS[i].0)
                .unwrap_or_else(|| default_colors(i))
        });
        App {
            tab: 0,
            row: 0,
            effect,
            colors,
            sel: 0,
            speed: s.get("speed").and_then(|v| v.parse().ok()).unwrap_or(5),
            brightness: s.get("brightness").and_then(|b| b.parse().ok()).unwrap_or(100),
            power: wmax::get_power_mode()
                .ok()
                .and_then(|m| wmax::POWER_MODES.iter().position(|(_, v)| *v as u32 == m))
                .unwrap_or(0),
            boost: wmax::fan_boost(0).unwrap_or(0) as u8,
            picker: None,
            status: String::new(),
        }
    }

    fn cur_colors(&self) -> &Vec<Rgb> {
        &self.colors[self.effect]
    }

    fn apply_led(&mut self) {
        let c = self.colors[self.effect].clone();
        let result = led::Led::open().and_then(|l| {
            l.brightness(self.brightness)?;
            match EFFECTS[self.effect] {
                "pulse" => l.pulse(c[0].0, c[0].1, c[0].2, self.speed),
                "morph" => l.morph(c[0], c[1], self.speed),
                "cycle" => l.cycle(&c, self.speed),
                "rainbow" => l.rainbow(&c, self.speed),
                _ => l.color(c[0].0, c[0].1, c[0].2),
            }
        });
        self.status = match result {
            Ok(()) => {
                let list: Vec<String> = c.iter().map(|&c| hex(c)).collect();
                let _ = state::set(&format!("colors_{}", EFFECTS[self.effect]), &list.join(","));
                let _ = state::set("effect", EFFECTS[self.effect]);
                let _ = state::set("speed", &self.speed.to_string());
                let _ = state::set("brightness", &self.brightness.to_string());
                format!("applied {}", EFFECTS[self.effect])
            }
            Err(e) => format!("led error: {e}"),
        };
    }

    fn apply_power(&mut self) {
        let (name, mode) = wmax::POWER_MODES[self.power];
        self.status = match wmax::set_power_mode(mode) {
            Ok(()) => {
                let _ = state::set("power", name);
                format!("power mode: {name}")
            }
            Err(e) => format!("power error: {e}"),
        };
    }

    fn apply_boost(&mut self) {
        self.status = match (0..2).try_for_each(|f| wmax::set_fan_boost(f, self.boost)) {
            Ok(()) => {
                let _ = state::set("boost", &self.boost.to_string());
                format!("fan boost: {}%", self.boost)
            }
            Err(e) => format!("fan error: {e}"),
        };
    }

    fn open_picker(&mut self) {
        self.picker = Some(Picker::from_rgb(self.cur_colors()[self.sel]));
    }

    fn picker_color(&self) -> Option<Rgb> {
        self.picker.as_ref().map(|p| hsv_to_rgb(p.h, p.s, p.v))
    }

    fn set_selected_color(&mut self, c: Rgb) {
        let sel = self.sel;
        self.colors[self.effect][sel] = c;
        self.apply_led();
    }

    fn screen_pick(&mut self) {
        // hyprpicker overlays the screen; the TUI just blocks until it returns.
        // Under sudo the Wayland env is stripped, so run it as the real user
        // with the runtime dir and socket restored.
        let out = match std::env::var("SUDO_USER").ok().zip(std::env::var("SUDO_UID").ok()) {
            Some((user, uid)) => {
                let run = format!("/run/user/{uid}");
                let sock = std::fs::read_dir(&run)
                    .ok()
                    .and_then(|d| {
                        d.filter_map(|e| e.ok())
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                            .find(|n| n.starts_with("wayland-") && !n.ends_with(".lock"))
                    })
                    .unwrap_or_else(|| "wayland-1".into());
                std::process::Command::new("sudo")
                    .args([
                        "-u",
                        &user,
                        "env",
                        &format!("XDG_RUNTIME_DIR={run}"),
                        &format!("WAYLAND_DISPLAY={sock}"),
                        "hyprpicker",
                        "--format=hex",
                    ])
                    .output()
            }
            None => std::process::Command::new("hyprpicker").arg("--format=hex").output(),
        };
        match out {
            Ok(o) if o.status.success() => {
                match parse_hex(String::from_utf8_lossy(&o.stdout).trim()) {
                    Some(c) => {
                        self.picker = Some(Picker::from_rgb(c));
                        self.set_selected_color(c);
                    }
                    None => self.status = "hyprpicker: no color picked".into(),
                }
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                self.status = format!("hyprpicker: {}", err.trim().lines().last().unwrap_or("cancelled"));
            }
            Err(e) => self.status = format!("hyprpicker failed: {e}"),
        }
    }

    fn adjust(&mut self, dir: i32) {
        let step = |v: u8| (v as i32 + dir * 10).clamp(0, 100) as u8;
        match (self.tab, self.row) {
            (0, 0) => {
                self.effect =
                    (self.effect as i32 + dir).rem_euclid(EFFECTS.len() as i32) as usize;
                self.sel = 0;
                self.apply_led();
            }
            (0, 1) => {
                let n = self.cur_colors().len() as i32;
                self.sel = (self.sel as i32 + dir).rem_euclid(n) as usize;
            }
            (0, 2) => {
                self.speed = (self.speed as i32 + dir).clamp(1, 10) as u8;
                self.apply_led();
            }
            (0, 3) => {
                self.brightness = step(self.brightness);
                self.apply_led();
            }
            (1, 0) => {
                self.power = (self.power as i32 + dir)
                    .rem_euclid(wmax::POWER_MODES.len() as i32) as usize;
                self.apply_power();
            }
            (1, 1) => {
                self.boost = step(self.boost);
                self.apply_boost();
            }
            _ => {}
        }
    }
}

fn slider(f: &mut Frame, area: Rect, selected: bool, label: &str, ratio: f64, text: String) {
    let style = if selected {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let marker = if selected { "▸ " } else { "  " };
    let [label_a, gauge_a, val_a] = Layout::horizontal([
        Constraint::Length(14),
        Constraint::Min(10),
        Constraint::Length(8),
    ])
    .areas(area);
    f.render_widget(Paragraph::new(format!("{marker}{label}")).style(style), label_a);
    f.render_widget(
        LineGauge::default()
            .filled_style(Style::default().fg(if selected { Color::Cyan } else { Color::Gray }))
            .ratio(ratio.clamp(0.0, 1.0))
            .label(""),
        gauge_a,
    );
    f.render_widget(Paragraph::new(format!(" {text}")).style(style), val_a);
}

fn text_row(selected: bool, label: &str, value: String) -> Line<'static> {
    let marker = if selected { "▸ " } else { "  " };
    let style = if selected {
        Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)
    } else {
        Style::default()
    };
    Line::styled(format!("{marker}{label:<12} ◂ {value} ▸"), style)
}

fn swatch_row(app: &App) -> Line<'static> {
    let selected_row = app.row == 1 && app.tab == 0;
    let marker = if selected_row { "▸ " } else { "  " };
    let mut spans = vec![Span::styled(
        format!("{marker}{:<12} ", "Colors"),
        if selected_row {
            Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)
        } else {
            Style::default()
        },
    )];
    for (i, &c) in app.cur_colors().iter().enumerate() {
        let block = Span::styled("██", Style::default().fg(Color::Rgb(c.0, c.1, c.2)));
        if i == app.sel && selected_row {
            spans.push(Span::styled("[", Style::default().fg(Color::Cyan)));
            spans.push(block);
            spans.push(Span::styled("]", Style::default().fg(Color::Cyan)));
        } else {
            spans.push(Span::raw(" "));
            spans.push(block);
            spans.push(Span::raw(" "));
        }
    }
    spans.push(Span::styled(
        format!("  #{}", hex(app.cur_colors()[app.sel.min(app.cur_colors().len() - 1)])),
        Style::default().fg(Color::DarkGray),
    ));
    Line::from(spans)
}

fn draw_picker(f: &mut Frame, app: &App) {
    let Some(p) = &app.picker else { return };
    let c = hsv_to_rgb(p.h, p.s, p.v);
    let area = {
        let [a] = Layout::horizontal([Constraint::Length(50)])
            .flex(Flex::Center)
            .areas(f.area());
        let [a] = Layout::vertical([Constraint::Length(10)]).flex(Flex::Center).areas(a);
        a
    };
    f.render_widget(Clear, area);
    let title = match &p.hex_input {
        Some(buf) => format!(" hex: #{buf}▏ "),
        None => format!(" color #{} ", hex(c)),
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let [preview_a, preset_a, hue_a, sat_a, val_a, _, help_a] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    f.render_widget(
        Paragraph::new("████████████████████████████████████")
            .style(Style::default().fg(Color::Rgb(c.0, c.1, c.2))),
        preview_a,
    );
    let preset_name = match PRESETS.iter().position(|&(_, pc)| pc == c) {
        Some(i) => PRESETS[i].0,
        None => "custom",
    };
    f.render_widget(
        Paragraph::new(text_row(p.row == 0, "Preset", preset_name.into())),
        preset_a,
    );
    slider(f, hue_a, p.row == 1, "Hue", p.h as f64 / 359.0, format!("{}°", p.h));
    slider(f, sat_a, p.row == 2, "Sat", p.s as f64 / 100.0, format!("{}%", p.s));
    slider(f, val_a, p.row == 3, "Val", p.v as f64 / 100.0, format!("{}%", p.v));
    f.render_widget(
        Paragraph::new("e hex · p pick on screen · Enter ok · Esc cancel")
            .style(Style::default().fg(Color::DarkGray)),
        help_a,
    );
}

fn section(title: &str, focused: bool) -> Block<'static> {
    let border = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(border)
        .title(format!(" {title} "))
}

fn draw(f: &mut Frame, app: &App, stats: &Option<hwmon::Stats>) {
    let [light_a, fan_a, stats_a, status_a] = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(1),
    ])
    .margin(1)
    .areas(f.area());

    // top section: backlight (focus with Tab or 1)
    let block = section("1:Backlight", app.tab == 0);
    let inner = block.inner(light_a);
    f.render_widget(block, light_a);
    let rows: [Rect; 4] = Layout::vertical([Constraint::Length(1); 4]).areas(inner);
    let focused = app.tab == 0;
    f.render_widget(
        Paragraph::new(text_row(focused && app.row == 0, "Effect", EFFECTS[app.effect].into())),
        rows[0],
    );
    f.render_widget(Paragraph::new(swatch_row(app)), rows[1]);
    if app.effect != 0 {
        slider(
            f,
            rows[2],
            focused && app.row == 2,
            "Speed",
            app.speed as f64 / 10.0,
            format!("{}/10", app.speed),
        );
    } else {
        f.render_widget(
            Paragraph::new(text_row(focused && app.row == 2, "Speed", "—".into())),
            rows[2],
        );
    }
    slider(
        f,
        rows[3],
        focused && app.row == 3,
        "Brightness",
        app.brightness as f64 / 100.0,
        format!("{}%", app.brightness),
    );

    // bottom section: fan & power
    let block = section("2:Fan & Power", app.tab == 1);
    let inner = block.inner(fan_a);
    f.render_widget(block, fan_a);
    let rows: [Rect; 2] = Layout::vertical([Constraint::Length(1); 2]).areas(inner);
    let focused = app.tab == 1;
    f.render_widget(
        Paragraph::new(text_row(
            focused && app.row == 0,
            "Power mode",
            wmax::POWER_MODES[app.power].0.into(),
        )),
        rows[0],
    );
    slider(
        f,
        rows[1],
        focused && app.row == 1,
        "Fan boost",
        app.boost as f64 / 100.0,
        format!("{}%", app.boost),
    );

    let stats_text = match stats {
        Some(s) => format!(
            "CPU {}°C   GPU {}°C\nfan1 {} rpm   fan2 {} rpm",
            s.cpu, s.gpu, s.fan1, s.fan2
        ),
        None => "sensors unavailable".into(),
    };
    f.render_widget(
        Paragraph::new(stats_text).block(Block::default().borders(Borders::ALL).title("sensors")),
        stats_a,
    );

    let help = if app.tab == 0 && app.row == 1 {
        "←→ swatch   Enter edit   a add   d delete   Tab switch   q quit"
    } else {
        "↑↓ select   ←→ change   Tab switch   q quit"
    };
    let status = if app.status.is_empty() {
        help.to_string()
    } else {
        format!("{}   ({help})", app.status)
    };
    f.render_widget(
        Paragraph::new(status).style(Style::default().fg(Color::DarkGray)),
        status_a,
    );

    draw_picker(f, app);
}

fn picker_key(app: &mut App, code: KeyCode) {
    let Some(p) = app.picker.as_mut() else { return };

    // hex entry mode captures everything until Enter/Esc
    if let Some(buf) = &mut p.hex_input {
        match code {
            KeyCode::Char(c) if c.is_ascii_hexdigit() && buf.len() < 6 => buf.push(c),
            KeyCode::Backspace => {
                buf.pop();
            }
            KeyCode::Enter => {
                if let Some(c) = parse_hex(buf) {
                    *p = Picker::from_rgb(c);
                    app.set_selected_color(c);
                } else {
                    p.hex_input = None;
                    app.status = "hex must be 6 digits".into();
                }
            }
            KeyCode::Esc => p.hex_input = None,
            _ => {}
        }
        return;
    }

    let live = match code {
        KeyCode::Up => {
            p.row = p.row.saturating_sub(1);
            false
        }
        KeyCode::Down => {
            p.row = (p.row + 1).min(3);
            false
        }
        KeyCode::Left | KeyCode::Right => {
            let d: i32 = if code == KeyCode::Left { -1 } else { 1 };
            match p.row {
                0 => {
                    // quick preset stepping — jumps straight to the named color
                    p.preset =
                        (p.preset as i32 + d).rem_euclid(PRESETS.len() as i32) as usize;
                    let (h, s, v) = rgb_to_hsv(PRESETS[p.preset].1);
                    (p.h, p.s, p.v) = (h, s, v);
                }
                1 => p.h = ((p.h as i32 + d * 10).rem_euclid(360)) as u16,
                2 => p.s = (p.s as i32 + d * 5).clamp(0, 100) as u8,
                _ => p.v = (p.v as i32 + d * 5).clamp(0, 100) as u8,
            }
            true
        }
        KeyCode::Char('e') | KeyCode::Char('#') => {
            p.hex_input = Some(String::new());
            false
        }
        KeyCode::Char('p') => {
            app.screen_pick();
            return;
        }
        KeyCode::Enter => {
            let c = app.picker_color().unwrap();
            app.picker = None;
            app.set_selected_color(c);
            return;
        }
        KeyCode::Esc => {
            app.picker = None;
            return;
        }
        _ => false,
    };
    if live {
        let c = app.picker_color().unwrap();
        app.set_selected_color(c); // live preview on the keyboard
    }
}

pub fn run() -> std::io::Result<()> {
    // Safety: plain isatty check on stdin
    if unsafe { libc::isatty(0) } == 0 {
        return Err(std::io::Error::other("g15 tui needs a terminal"));
    }
    let mut app = App::from_state();
    let mut terminal = ratatui::init();
    let result = loop {
        let stats = hwmon::read().ok();
        if let Err(e) = terminal.draw(|f| draw(f, &app, &stats)) {
            break Err(e);
        }
        if event::poll(Duration::from_secs(1)).unwrap_or(false) {
            match event::read() {
                Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => {
                    if app.picker.is_some() {
                        picker_key(&mut app, k.code);
                        continue;
                    }
                    let rows = if app.tab == 0 { 4 } else { 2 };
                    match k.code {
                        KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                        KeyCode::Tab | KeyCode::BackTab => {
                            app.tab ^= 1;
                            app.row = 0;
                        }
                        KeyCode::Char('1') => app.tab = 0,
                        KeyCode::Char('2') => app.tab = 1,
                        KeyCode::Up => app.row = app.row.saturating_sub(1),
                        KeyCode::Down => app.row = (app.row + 1).min(rows - 1),
                        KeyCode::Left => app.adjust(-1),
                        KeyCode::Right => app.adjust(1),
                        KeyCode::Enter if app.tab == 0 && app.row == 1 => app.open_picker(),
                        KeyCode::Enter => app.adjust(1),
                        KeyCode::Char('a') if app.tab == 0 && app.row == 1 => {
                            let max = COLOR_LIMITS[app.effect].1;
                            if app.colors[app.effect].len() < max {
                                let c = app.cur_colors()[app.sel];
                                app.colors[app.effect].insert(app.sel + 1, c);
                                app.sel += 1;
                                app.apply_led();
                            } else {
                                app.status = format!("{} allows at most {max} colors", EFFECTS[app.effect]);
                            }
                        }
                        KeyCode::Char('d') | KeyCode::Char('x')
                            if app.tab == 0 && app.row == 1 =>
                        {
                            let min = COLOR_LIMITS[app.effect].0;
                            if app.colors[app.effect].len() > min {
                                app.colors[app.effect].remove(app.sel);
                                app.sel = app.sel.min(app.colors[app.effect].len() - 1);
                                app.apply_led();
                            } else {
                                app.status = format!("{} needs at least {min} colors", EFFECTS[app.effect]);
                            }
                        }
                        _ => {}
                    }
                }
                Ok(_) => {}
                Err(e) => break Err(e),
            }
        }
    };
    ratatui::restore();
    result
}
