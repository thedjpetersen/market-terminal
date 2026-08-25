use ratatui::style::{Color, Style};

pub const BG: Color = Color::Rgb(2, 3, 3);
pub const INK: Color = Color::Rgb(222, 221, 215);
pub const MUTED: Color = Color::Rgb(124, 128, 128);
pub const AMBER: Color = Color::Rgb(242, 173, 55);
pub const YELLOW: Color = Color::Rgb(226, 217, 103);
pub const CYAN: Color = Color::Rgb(99, 212, 237);
pub const GREEN: Color = Color::Rgb(158, 229, 79);
pub const RED: Color = Color::Rgb(241, 69, 112);
pub const NAV_BG: Color = Color::Rgb(21, 32, 35);
pub const FOOTER_BG: Color = Color::Rgb(40, 52, 54);

pub fn value(value: &str) -> Style {
    if value.starts_with('+') {
        Style::new().fg(GREEN)
    } else if value.starts_with('−') || value.starts_with('-') {
        Style::new().fg(RED)
    } else {
        Style::new().fg(INK)
    }
}
