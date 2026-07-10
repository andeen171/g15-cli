//! Two-tab TUI (Backlight / Fan & Power), omarchy-style. Changes apply live.
//! Run as root (the g15-tui wrapper sudo's) so WMAX works; LED works either way.

use crate::{hwmon, led, state, wmax};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Tabs};
use ratatui::Frame;
use std::time::Duration;

const COLORS: [(&str, u32); 9] = [
    ("white", 0xFFFFFF),
    ("red", 0xFF0000),
    ("orange", 0xFF8800),
    ("yellow", 0xFFFF00),
    ("green", 0x00FF00),
    ("cyan", 0x00FFFF),
    ("blue", 0x0066FF),
    ("purple", 0x8800FF),
    ("pink", 0xFF00AA),
];
const EFFECTS: [&str; 3] = ["static", "pulse", "morph"];

struct App {
    tab: usize,
    row: usize,
    color: usize,
    effect: usize,
    brightness: u8,
    power: usize,
    boost: u8,
    status: String,
}

fn rgb(idx: usize) -> (u8, u8, u8) {
    let v = COLORS[idx].1;
    ((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl App {
    fn from_state() -> Self {
        let s = state::load();
        let color = s
            .get("color")
            .and_then(|c| u32::from_str_radix(c, 16).ok())
            .and_then(|v| COLORS.iter().position(|(_, hex)| *hex == v))
            .unwrap_or(0);
        let effect = s
            .get("effect")
            .and_then(|e| EFFECTS.iter().position(|n| n == e))
            .unwrap_or(0);
        let brightness = s.get("brightness").and_then(|b| b.parse().ok()).unwrap_or(100);
        let power = wmax::get_power_mode()
            .ok()
            .and_then(|m| wmax::POWER_MODES.iter().position(|(_, v)| *v as u32 == m))
            .unwrap_or(0);
        let boost = wmax::fan_boost(0).unwrap_or(0) as u8;
        App { tab: 0, row: 0, color, effect, brightness, power, boost, status: String::new() }
    }

    fn apply_led(&mut self) {
        let result = led::Led::open().and_then(|l| {
            l.brightness(self.brightness)?;
            let c = rgb(self.color);
            match EFFECTS[self.effect] {
                "pulse" => l.pulse(c.0, c.1, c.2),
                "morph" => l.morph(c, rgb((self.color + 3) % COLORS.len())),
                _ => l.color(c.0, c.1, c.2),
            }
        });
        self.status = match result {
            Ok(()) => {
                let _ = state::set("color", &format!("{:06x}", COLORS[self.color].1));
                let _ = state::set(
                    "color2",
                    &format!("{:06x}", COLORS[(self.color + 3) % COLORS.len()].1),
                );
                let _ = state::set("effect", EFFECTS[self.effect]);
                let _ = state::set("brightness", &self.brightness.to_string());
                format!("applied {} {}", EFFECTS[self.effect], COLORS[self.color].0)
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

    fn adjust(&mut self, dir: i32) {
        let step = |v: u8| (v as i32 + dir * 10).clamp(0, 100) as u8;
        let cycle = |v: usize, n: usize| (v as i32 + dir).rem_euclid(n as i32) as usize;
        match (self.tab, self.row) {
            (0, 0) => {
                self.color = cycle(self.color, COLORS.len());
                self.apply_led();
            }
            (0, 1) => {
                self.effect = cycle(self.effect, EFFECTS.len());
                self.apply_led();
            }
            (0, 2) => {
                self.brightness = step(self.brightness);
                self.apply_led();
            }
            (1, 0) => {
                self.power = cycle(self.power, wmax::POWER_MODES.len());
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

fn row_line(selected: bool, label: &str, value: String) -> Line<'static> {
    let marker = if selected { "▸ " } else { "  " };
    let style = if selected {
        Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan)
    } else {
        Style::default()
    };
    Line::styled(format!("{marker}{label:<12} ◂ {value} ▸"), style)
}

fn draw(f: &mut Frame, app: &App, stats: &Option<hwmon::Stats>) {
    let [tabs_a, body_a, gauge_a, stats_a, status_a] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(5),
        Constraint::Length(3),
        Constraint::Length(4),
        Constraint::Length(1),
    ])
    .margin(1)
    .areas(f.area());

    f.render_widget(
        Tabs::new(["1:Backlight", "2:Fan & Power"])
            .select(app.tab)
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        tabs_a,
    );

    let (rows, gauge_val, gauge_label): (Vec<Line>, u8, &str) = if app.tab == 0 {
        (
            vec![
                row_line(app.row == 0, "Color", COLORS[app.color].0.into()),
                row_line(app.row == 1, "Effect", EFFECTS[app.effect].into()),
                row_line(app.row == 2, "Brightness", format!("{}%", app.brightness)),
            ],
            app.brightness,
            "brightness",
        )
    } else {
        (
            vec![
                row_line(app.row == 0, "Power mode", wmax::POWER_MODES[app.power].0.into()),
                row_line(app.row == 1, "Fan boost", format!("{}%", app.boost)),
            ],
            app.boost,
            "fan boost",
        )
    };
    f.render_widget(
        Paragraph::new(rows).block(Block::default().borders(Borders::ALL)),
        body_a,
    );
    f.render_widget(
        Gauge::default()
            .block(Block::default().borders(Borders::ALL).title(gauge_label))
            .gauge_style(Style::default().fg(Color::Cyan))
            .percent(gauge_val as u16),
        gauge_a,
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

    let help = "↑↓ select   ←→ change   Tab switch   q quit";
    let status = if app.status.is_empty() { help.into() } else { format!("{}   ({help})", app.status) };
    f.render_widget(Paragraph::new(status).style(Style::default().fg(Color::DarkGray)), status_a);
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
                    let rows = if app.tab == 0 { 3 } else { 2 };
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
                        KeyCode::Right | KeyCode::Enter => app.adjust(1),
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
