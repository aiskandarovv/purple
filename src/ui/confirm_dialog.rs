use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, _app: &App, alias: &str) {

    let area = super::centered_rect_fixed(48, 7, frame.area());

    // Clear background
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(Span::styled(" Confirm Delete ", theme::danger()))
        .borders(Borders::ALL)
        .border_style(theme::border_danger());

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  Delete \"{}\"?", alias),
            theme::bold(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("    y", theme::danger()),
            Span::styled(" yes   ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" no", theme::muted()),
        ]),
    ];

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render_host_key_reset(frame: &mut Frame, _app: &App, hostname: &str) {
    let display = super::truncate(hostname, 40);
    let area = super::centered_rect_fixed(52, 9, frame.area());

    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(Span::styled(" Host Key Changed ", theme::danger()))
        .borders(Borders::ALL)
        .border_style(theme::border_danger());

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  Host key for {} changed.", display),
            theme::bold(),
        )),
        Line::from(Span::styled(
            "  This can happen after a server reinstall.",
            theme::muted(),
        )),
        Line::from(Span::styled(
            "  Remove old key and reconnect?",
            theme::muted(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("    y", theme::accent_bold()),
            Span::styled(" yes   ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" no", theme::muted()),
        ]),
    ];

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}
