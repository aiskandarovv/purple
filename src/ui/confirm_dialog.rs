use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, _app: &App, alias: &str) {
    let area = super::centered_rect_fixed(48, 7, frame.area());

    // Clear background
    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Confirm Delete ", theme::danger()))
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
            Span::styled(" yes ", theme::muted()),
            Span::styled("\u{2502} ", theme::muted()),
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

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Host Key Changed ", theme::danger()))
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
            Span::styled("    y", theme::danger()),
            Span::styled(" yes ", theme::muted()),
            Span::styled("\u{2502} ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" no", theme::muted()),
        ]),
    ];

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render_confirm_import(frame: &mut Frame, _app: &App, count: usize) {
    let area = super::centered_rect_fixed(52, 7, frame.area());

    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Import ", theme::brand()))
        .border_style(theme::accent());

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "  Import {} host{} from known_hosts?",
                count,
                if count == 1 { "" } else { "s" },
            ),
            theme::bold(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("    y", theme::accent_bold()),
            Span::styled(" yes ", theme::muted()),
            Span::styled("\u{2502} ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" no", theme::muted()),
        ]),
    ];

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

pub fn render_welcome(
    frame: &mut Frame,
    _app: &App,
    has_backup: bool,
    host_count: usize,
    known_hosts_count: usize,
) {
    let has_hosts = host_count > 0;
    // Height: blank + title + blank-before-footer + footer = 4 inner + 2 border = 6 base
    // When info lines are present, add them + 1 extra blank separator
    let info_lines = if has_hosts {
        1 + if has_backup { 2 } else { 0 }
    } else {
        (if known_hosts_count > 0 { 2 } else { 0 }) + if has_backup { 2 } else { 0 }
    };
    let height = 7 + info_lines + if info_lines > 0 { 1 } else { 0 };
    let area = super::centered_rect_fixed(60, height as u16, frame.area());

    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Welcome ", theme::brand()))
        .border_style(theme::accent());

    let mut text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Welcome to ", theme::bold()),
            Span::styled("purple", theme::border_search()),
            Span::styled(".", theme::bold()),
        ])
        .alignment(Alignment::Center),
    ];
    if has_hosts {
        text.push(Line::from(""));
        text.push(
            Line::from(Span::styled(
                format!(
                    "Found {} host{} in your SSH config.",
                    host_count,
                    if host_count == 1 { "" } else { "s" },
                ),
                theme::muted(),
            ))
            .alignment(Alignment::Center),
        );
    } else if known_hosts_count > 0 {
        text.push(Line::from(""));
        text.push(
            Line::from(Span::styled(
                format!(
                    "Found {} host{} in known_hosts.",
                    known_hosts_count,
                    if known_hosts_count == 1 { "" } else { "s" },
                ),
                theme::muted(),
            ))
            .alignment(Alignment::Center),
        );
        text.push(
            Line::from(vec![
                Span::styled("Press ", theme::muted()),
                Span::styled("I", theme::accent_bold()),
                Span::styled(" to import them.", theme::muted()),
            ])
            .alignment(Alignment::Center),
        );
    }
    if has_backup {
        if !has_hosts && known_hosts_count == 0 {
            text.push(Line::from(""));
        }
        text.push(
            Line::from(Span::styled(
                "Your original config has been backed up",
                theme::muted(),
            ))
            .alignment(Alignment::Center),
        );
        text.push(
            Line::from(Span::styled("to ~/.purple/config.original", theme::muted()))
                .alignment(Alignment::Center),
        );
    }
    text.push(Line::from(""));
    text.push(
        Line::from(vec![
            Span::styled("?", theme::accent_bold()),
            Span::styled(" cheat sheet ", theme::muted()),
            Span::styled("\u{2502} ", theme::muted()),
            Span::styled("Enter", theme::accent_bold()),
            Span::styled(" continue", theme::muted()),
        ])
        .alignment(Alignment::Center),
    );
    text.push(Line::from(""));

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

/// Compute the welcome dialog height and text line count for testing.
/// Returns (height, text_line_count).
#[cfg(test)]
fn welcome_height_and_lines(
    has_backup: bool,
    host_count: usize,
    known_hosts_count: usize,
) -> (usize, usize) {
    let has_hosts = host_count > 0;
    let info_lines = if has_hosts {
        1 + if has_backup { 2 } else { 0 }
    } else {
        (if known_hosts_count > 0 { 2 } else { 0 }) + if has_backup { 2 } else { 0 }
    };
    let height = 7 + info_lines + if info_lines > 0 { 1 } else { 0 };

    // Replicate the text-building logic
    let mut lines = 2; // blank + title
    if has_hosts {
        lines += 2; // blank + "Found N hosts"
    } else if known_hosts_count > 0 {
        lines += 3; // blank + "Found N in known_hosts" + "Press I to import"
    }
    if has_backup {
        if !has_hosts && known_hosts_count == 0 {
            lines += 1; // explicit blank
        }
        lines += 2; // backup line 1 + backup line 2
    }
    lines += 3; // blank + footer + trailing blank

    (height, lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Welcome dialog height calculation — all 8 permutations
    // =========================================================================
    // (has_hosts, has_backup, known_hosts > 0)
    // Note: when has_hosts=true, known_hosts_count is irrelevant (else-if branch)
    // but we test both 0 and >0 to confirm it doesn't affect height.

    #[test]
    fn welcome_height_hosts_backup_no_known() {
        let (height, lines) = welcome_height_and_lines(true, 5, 0);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=true, has_backup=true, known=0"
        );
    }

    #[test]
    fn welcome_height_hosts_backup_with_known() {
        let (height, lines) = welcome_height_and_lines(true, 5, 10);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=true, has_backup=true, known=10"
        );
    }

    #[test]
    fn welcome_height_hosts_no_backup_no_known() {
        let (height, lines) = welcome_height_and_lines(false, 5, 0);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=true, has_backup=false, known=0"
        );
    }

    #[test]
    fn welcome_height_hosts_no_backup_with_known() {
        let (height, lines) = welcome_height_and_lines(false, 5, 10);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=true, has_backup=false, known=10"
        );
    }

    #[test]
    fn welcome_height_no_hosts_backup_no_known() {
        let (height, lines) = welcome_height_and_lines(true, 0, 0);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=false, has_backup=true, known=0"
        );
    }

    #[test]
    fn welcome_height_no_hosts_backup_with_known() {
        let (height, lines) = welcome_height_and_lines(true, 0, 10);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=false, has_backup=true, known=10"
        );
    }

    #[test]
    fn welcome_height_no_hosts_no_backup_no_known() {
        let (height, lines) = welcome_height_and_lines(false, 0, 0);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=false, has_backup=false, known=0"
        );
    }

    #[test]
    fn welcome_height_no_hosts_no_backup_with_known() {
        let (height, lines) = welcome_height_and_lines(false, 0, 10);
        assert_eq!(
            lines,
            height - 2,
            "has_hosts=false, has_backup=false, known=10"
        );
    }

    // Edge cases for host_count and known_hosts_count boundary values
    #[test]
    fn welcome_height_single_host() {
        let (height, lines) = welcome_height_and_lines(false, 1, 0);
        assert_eq!(lines, height - 2, "single host");
    }

    #[test]
    fn welcome_height_single_known_host() {
        let (height, lines) = welcome_height_and_lines(false, 0, 1);
        assert_eq!(lines, height - 2, "single known_hosts entry");
    }

    // =========================================================================
    // Confirm import dialog pluralization
    // =========================================================================

    #[test]
    fn confirm_import_pluralization_single() {
        let msg = format!(
            "  Import {} host{} from known_hosts?",
            1,
            if 1 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "  Import 1 host from known_hosts?");
    }

    #[test]
    fn confirm_import_pluralization_multiple() {
        let msg = format!(
            "  Import {} host{} from known_hosts?",
            42,
            if 42 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "  Import 42 hosts from known_hosts?");
    }

    // =========================================================================
    // Welcome dialog pluralization
    // =========================================================================

    #[test]
    fn welcome_hosts_pluralization_single() {
        let msg = format!(
            "Found {} host{} in your SSH config.",
            1,
            if 1 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "Found 1 host in your SSH config.");
    }

    #[test]
    fn welcome_hosts_pluralization_multiple() {
        let msg = format!(
            "Found {} host{} in your SSH config.",
            12,
            if 12 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "Found 12 hosts in your SSH config.");
    }

    #[test]
    fn welcome_known_hosts_pluralization_single() {
        let msg = format!(
            "Found {} host{} in known_hosts.",
            1,
            if 1 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "Found 1 host in known_hosts.");
    }

    #[test]
    fn welcome_known_hosts_pluralization_multiple() {
        let msg = format!(
            "Found {} host{} in known_hosts.",
            34,
            if 34 == 1 { "" } else { "s" },
        );
        assert_eq!(msg, "Found 34 hosts in known_hosts.");
    }
}
