//! Runtime color presets adapted from `makeev/alphai-tui` commit
//! `9143d2e1176d0a67a9f26960427cf370187fc2e6` (MIT, Copyright (c) 2026
//! Mikhail Makeev). The semantic slots and runtime resolver are specific to
//! Market Terminal; see `THIRD_PARTY_NOTICES.md`.

use std::cell::Cell;

use ratatui::style::{Color, Style};

const SLOT_COUNT: usize = 10;
const DEFAULT_PRESET: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThemeColor(usize);

pub const BG: ThemeColor = ThemeColor(0);
pub const INK: ThemeColor = ThemeColor(1);
pub const MUTED: ThemeColor = ThemeColor(2);
pub const AMBER: ThemeColor = ThemeColor(3);
pub const YELLOW: ThemeColor = ThemeColor(4);
pub const CYAN: ThemeColor = ThemeColor(5);
pub const GREEN: ThemeColor = ThemeColor(6);
pub const RED: ThemeColor = ThemeColor(7);
pub const NAV_BG: ThemeColor = ThemeColor(8);
pub const FOOTER_BG: ThemeColor = ThemeColor(9);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Palette {
    colors: [Color; SLOT_COUNT],
}

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn palette(colors: [u32; SLOT_COUNT]) -> Palette {
    Palette {
        colors: [
            rgb(colors[0]),
            rgb(colors[1]),
            rgb(colors[2]),
            rgb(colors[3]),
            rgb(colors[4]),
            rgb(colors[5]),
            rgb(colors[6]),
            rgb(colors[7]),
            rgb(colors[8]),
            rgb(colors[9]),
        ],
    }
}

/// Presets are ordered for cycling: dark palettes first, then light palettes.
/// Slots are background, text, muted, accent, yellow, cyan, green, red,
/// navigation background, and footer background.
const PRESETS: &[(&str, Palette)] = &[
    (
        DEFAULT_PRESET,
        palette([
            0x020303, 0xdeddd7, 0x7c8080, 0xf2ad37, 0xe2d967, 0x63d4ed, 0x9ee54f, 0xf14570,
            0x152023, 0x283436,
        ]),
    ),
    (
        "catppuccin-mocha",
        palette([
            0x1e1e2e, 0xcdd6f4, 0x7f849c, 0xcba6f7, 0xf9e2af, 0x89dceb, 0xa6e3a1, 0xf38ba8,
            0x181825, 0x313244,
        ]),
    ),
    (
        "catppuccin-macchiato",
        palette([
            0x24273a, 0xcad3f5, 0x8087a2, 0xc6a0f6, 0xeed49f, 0x91d7e3, 0xa6da95, 0xed8796,
            0x1e2030, 0x363a4f,
        ]),
    ),
    (
        "catppuccin-frappe",
        palette([
            0x303446, 0xc6d0f5, 0x838ba7, 0xca9ee6, 0xe5c890, 0x99d1db, 0xa6d189, 0xe78284,
            0x292c3c, 0x414559,
        ]),
    ),
    (
        "dracula",
        palette([
            0x282a36, 0xf8f8f2, 0x6272a4, 0xbd93f9, 0xf1fa8c, 0x8be9fd, 0x50fa7b, 0xff5555,
            0x21222c, 0x44475a,
        ]),
    ),
    (
        "gruvbox-dark",
        palette([
            0x282828, 0xebdbb2, 0x928374, 0x83a598, 0xfabd2f, 0x8ec07c, 0xb8bb26, 0xfb4934,
            0x1d2021, 0x3c3836,
        ]),
    ),
    (
        "nord",
        palette([
            0x2e3440, 0xd8dee9, 0x616e88, 0x88c0d0, 0xebcb8b, 0x8fbcbb, 0xa3be8c, 0xbf616a,
            0x3b4252, 0x434c5e,
        ]),
    ),
    (
        "catppuccin-latte",
        palette([
            0xeff1f5, 0x4c4f69, 0x8c8fa1, 0x8839ef, 0xdf8e1d, 0x04a5e5, 0x40a02b, 0xd20f39,
            0xe6e9ef, 0xccd0da,
        ]),
    ),
    (
        "gruvbox-light",
        palette([
            0xfbf1c7, 0x3c3836, 0x928374, 0x076678, 0xb57614, 0x427b58, 0x79740e, 0x9d0006,
            0xf2e5bc, 0xd5c4a1,
        ]),
    ),
];

thread_local! {
    /// Crossterm renders and handles input on one thread. Thread-local state
    /// keeps independently running render tests from changing each other's
    /// palette while retaining lock-free runtime lookups.
    static ACTIVE_PRESET: Cell<u8> = const { Cell::new(0) };
}

impl ThemeColor {
    fn resolve(self) -> Color {
        let preset = active_preset_index();
        PRESETS[preset].1.colors[self.0]
    }
}

impl From<ThemeColor> for Color {
    fn from(value: ThemeColor) -> Self {
        value.resolve()
    }
}

impl From<ThemeColor> for Style {
    fn from(value: ThemeColor) -> Self {
        Style::new().fg(value.into())
    }
}

pub(crate) fn active_theme_name() -> &'static str {
    PRESETS[active_preset_index()].0
}

/// Activates a preset by name, case-insensitively, returning its canonical name.
pub(crate) fn set_theme(name: &str) -> Option<&'static str> {
    let index = PRESETS
        .iter()
        .position(|(candidate, _)| candidate.eq_ignore_ascii_case(name.trim()))?;
    ACTIVE_PRESET.set(index as u8);
    Some(PRESETS[index].0)
}

pub(crate) fn cycle_theme(direction: isize) -> &'static str {
    let count = PRESETS.len() as isize;
    let current = active_preset_index() as isize;
    let next = (current + direction).rem_euclid(count) as usize;
    ACTIVE_PRESET.set(next as u8);
    PRESETS[next].0
}

fn active_preset_index() -> usize {
    usize::from(ACTIVE_PRESET.get()).min(PRESETS.len().saturating_sub(1))
}

pub(crate) fn preset_names() -> impl Iterator<Item = &'static str> {
    PRESETS.iter().map(|(name, _)| *name)
}

pub fn value(value: &str) -> Style {
    if value.starts_with('+') {
        Style::new().fg(GREEN.into())
    } else if value.starts_with('−') || value.starts_with('-') {
        Style::new().fg(RED.into())
    } else {
        Style::new().fg(INK.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_is_case_and_space_tolerant() {
        let original = active_theme_name();
        assert_eq!(set_theme(" Catppuccin-Mocha "), Some("catppuccin-mocha"));
        assert!(set_theme("catppuccino").is_none());
        set_theme(original).expect("restore original theme");
    }

    #[test]
    fn cycle_reaches_every_preset_and_wraps() {
        let original = active_theme_name();
        set_theme(DEFAULT_PRESET).expect("default preset");
        let names = preset_names().collect::<Vec<_>>();
        let mut seen = vec![active_theme_name()];
        for _ in 1..names.len() {
            seen.push(cycle_theme(1));
        }
        assert_eq!(seen, names);
        assert_eq!(cycle_theme(1), DEFAULT_PRESET);
        assert_eq!(cycle_theme(-1), names[names.len() - 1]);
        set_theme(original).expect("restore original theme");
    }
}
