use std::sync::atomic::{AtomicU8, Ordering};

use ratatui::style::{Color, Modifier, Style};

/// Color mode: 0 = NO_COLOR, 1 = ANSI 16, 2 = truecolor.
static COLOR_MODE: AtomicU8 = AtomicU8::new(1);

/// Initialize theme settings. Call once at startup.
pub fn init() {
    if std::env::var_os("NO_COLOR").is_some() {
        COLOR_MODE.store(0, Ordering::Release);
    } else if std::env::var("COLORTERM")
        .map(|v| v == "truecolor" || v == "24bit")
        .unwrap_or(false)
    {
        COLOR_MODE.store(2, Ordering::Release);
    }
}

/// Current color mode: 0 = NO_COLOR, 1 = ANSI 16, 2 = truecolor.
pub fn color_mode() -> u8 {
    COLOR_MODE.load(Ordering::Acquire)
}

/// Brand badge: purple background with white text. The single splash of color.
/// Truecolor: #9333EA purple bg. ANSI 16: Magenta bg. NO_COLOR: REVERSED.
/// Removes DIM so border_style doesn't leak through ratatui's Style::patch().
pub fn brand_badge() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default()
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            .remove_modifier(Modifier::DIM),
        2 => Style::default()
            .fg(Color::White)
            .bg(Color::Rgb(147, 51, 234))
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM),
        _ => Style::default()
            .fg(Color::White)
            .bg(Color::Magenta)
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM),
    }
}

/// Brand accent for dialog/popup titles.
/// Removes DIM so border_style doesn't leak through ratatui's Style::patch().
pub fn brand() -> Style {
    Style::default()
        .add_modifier(Modifier::BOLD)
        .remove_modifier(Modifier::DIM)
}

/// Structural elements (overlay borders, tags).
pub fn accent() -> Style {
    Style::default()
}

/// Keybinding keys in footer/help.
pub fn accent_bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Search match highlight.
pub fn highlight_bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED)
}

/// Footer keycap style: background matches the dim border tone.
/// Truecolor: explicit gray bg matching typical DIM rendering.
/// ANSI 16: DarkGray bg approximates DIM borders.
/// NO_COLOR: REVERSED fallback.
pub fn footer_key() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default().add_modifier(Modifier::REVERSED),
        2 => Style::default().fg(Color::White).bg(Color::Rgb(88, 88, 88)),
        _ => Style::default().fg(Color::White).bg(Color::DarkGray),
    }
}

/// Muted/secondary text.
pub fn muted() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Section headers (help overlay, host detail).
pub fn section_header() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Error message. Red when color is available.
pub fn error() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default().add_modifier(Modifier::BOLD),
        2 => Style::default()
            .fg(Color::Rgb(239, 68, 68))
            .add_modifier(Modifier::BOLD),
        _ => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    }
}

/// Success message. Green when color is available.
pub fn success() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default().add_modifier(Modifier::BOLD),
        2 => Style::default()
            .fg(Color::Rgb(34, 197, 94))
            .add_modifier(Modifier::BOLD),
        _ => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    }
}

/// Style for online status dot. Three urgency tiers:
/// NO_COLOR = normal (no modifier), ANSI 16 = Green + DIM, truecolor = muted green + DIM.
pub fn online_dot() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default(), // normal (no modifier)
        2 => Style::default()
            .fg(Color::Rgb(34, 197, 94))
            .add_modifier(Modifier::DIM),
        _ => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::DIM),
    }
}

/// Warning message. Yellow/amber when color is available.
pub fn warning() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default().add_modifier(Modifier::BOLD),
        2 => Style::default()
            .fg(Color::Rgb(234, 179, 8))
            .add_modifier(Modifier::BOLD),
        _ => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    }
}

/// Danger action key (delete confirmation). Red when color is available.
pub fn danger() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default().add_modifier(Modifier::BOLD),
        2 => Style::default()
            .fg(Color::Rgb(239, 68, 68))
            .add_modifier(Modifier::BOLD),
        _ => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    }
}

/// Default border (unfocused).
pub fn border() -> Style {
    Style::default().add_modifier(Modifier::DIM)
}

/// Version number in help overlay. Purple foreground.
pub fn version() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default().add_modifier(Modifier::BOLD),
        2 => Style::default().fg(Color::Rgb(147, 51, 234)),
        _ => Style::default().fg(Color::Magenta),
    }
}

/// Search-mode border. Purple to signal active filter state.
pub fn border_search() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default().add_modifier(Modifier::BOLD),
        2 => Style::default().fg(Color::Rgb(147, 51, 234)),
        _ => Style::default().fg(Color::Magenta),
    }
}

/// Selected item in a list. Purple highlight for brand consistency.
pub fn selected_row() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default()
            .add_modifier(Modifier::REVERSED)
            .remove_modifier(Modifier::DIM),
        2 => Style::default()
            .fg(Color::White)
            .bg(Color::Rgb(147, 51, 234))
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM),
        _ => Style::default()
            .fg(Color::White)
            .bg(Color::Magenta)
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM),
    }
}

/// Danger border (delete dialog). Red when color is available.
pub fn border_danger() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default().add_modifier(Modifier::BOLD),
        2 => Style::default()
            .fg(Color::Rgb(239, 68, 68))
            .add_modifier(Modifier::BOLD),
        _ => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    }
}

/// Bold text (labels, emphasis).
pub fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// Update available badge. Purple background to stand out in the title bar.
pub fn update_badge() -> Style {
    match COLOR_MODE.load(Ordering::Acquire) {
        0 => Style::default()
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            .remove_modifier(Modifier::DIM),
        2 => Style::default()
            .fg(Color::White)
            .bg(Color::Rgb(147, 51, 234))
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM),
        _ => Style::default()
            .fg(Color::White)
            .bg(Color::Magenta)
            .add_modifier(Modifier::BOLD)
            .remove_modifier(Modifier::DIM),
    }
}
