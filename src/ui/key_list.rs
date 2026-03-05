use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App) {
    let title = if app.keys.is_empty() {
        Span::styled(" SSH Keys ", theme::brand())
    } else {
        let pos = app.ui.key_list_state.selected().map(|i| i + 1).unwrap_or(0);
        Span::styled(format!(" SSH Keys {}/{} ", pos, app.keys.len()), theme::brand())
    };

    // Overlay: percentage-based width, height fits content
    let item_count = app.keys.len().max(1);
    let height = (item_count as u16 + 6).min(frame.area().height.saturating_sub(4));
    let area = {
        let r = super::centered_rect(70, 80, frame.area());
        super::centered_rect_fixed(r.width, height, frame.area())
    };
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.keys.is_empty() {
        let msg = Paragraph::new("  No keys found in ~/.ssh/. Try ssh-keygen to forge one.")
            .style(theme::muted());
        frame.render_widget(msg, inner);
        return;
    }

    // Fixed columns: name(16) + type(10) + hosts(8) = 34 + leading space
    // Comment gets remaining space (placed last so it can truncate)
    let content_width = inner.width as usize;
    let comment_width = content_width.saturating_sub(1 + 16 + 10 + 8);

    let items: Vec<ListItem> = app
        .keys
        .iter()
        .map(|key| {
            let type_display = key.type_display();

            let host_count = key.linked_hosts.len();
            let host_label = match host_count {
                0 => "0 hosts".to_string(),
                1 => "1 host".to_string(),
                n => format!("{} hosts", n),
            };

            let comment_display = if key.comment.is_empty() {
                String::new()
            } else {
                super::truncate(&key.comment, comment_width.saturating_sub(1))
            };

            let line = Line::from(vec![
                Span::styled(format!(" {:<16}", key.name), theme::bold()),
                Span::styled(format!("{:<10}", type_display), theme::muted()),
                Span::styled(format!("{:<8}", host_label), theme::muted()),
                Span::styled(comment_display, theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let inner_chunks = Layout::vertical([
        Constraint::Length(1), // Column header
        Constraint::Min(1),   // List
        Constraint::Length(1), // Footer
    ])
    .split(inner);

    // Column header
    let header = Line::from(vec![
        Span::styled(format!(" {:<16}", "NAME"), theme::muted()),
        Span::styled(format!("{:<10}", "TYPE"), theme::muted()),
        Span::styled(format!("{:<8}", "HOSTS"), theme::muted()),
        Span::styled("COMMENT", theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(header), inner_chunks[0]);

    let list = List::new(items)
        .highlight_style(theme::selected())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, inner_chunks[1], &mut app.ui.key_list_state);

    // Footer
    let footer = Line::from(vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" details  ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" back", theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(footer), inner_chunks[2]);
}
