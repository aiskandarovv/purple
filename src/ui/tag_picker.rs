use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App) {
    if app.tag_list.is_empty() {
        let area = super::centered_rect_fixed(50, 5, frame.area());
        frame.render_widget(Clear, area);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Span::styled(" Filter by Tag ", theme::brand()))
            .border_style(theme::accent());
        let msg = Paragraph::new(Line::from(Span::styled(
            "  No tags yet. Press t on a host to add some.",
            theme::muted(),
        )))
        .block(block);
        frame.render_widget(msg, area);
        return;
    }

    // Count hosts per tag (including provider as virtual tag)
    let tag_counts: std::collections::HashMap<&str, usize> = {
        let mut counts = std::collections::HashMap::new();
        for host in &app.hosts {
            for tag in host.provider_tags.iter().chain(host.tags.iter()) {
                *counts.entry(tag.as_str()).or_insert(0) += 1;
            }
            if let Some(ref provider) = host.provider {
                *counts.entry(provider.as_str()).or_insert(0) += 1;
            }
            if host.stale.is_some()
                && !host
                    .tags
                    .iter()
                    .chain(host.provider_tags.iter())
                    .any(|t| t.eq_ignore_ascii_case("stale"))
            {
                *counts.entry("stale").or_insert(0) += 1;
            }
        }
        counts
    };

    let height = (app.tag_list.len() as u16 + 5).min(frame.area().height.saturating_sub(4));
    let area = super::centered_rect_fixed(50, height, frame.area());
    frame.render_widget(Clear, area);

    let items: Vec<ListItem> = app
        .tag_list
        .iter()
        .map(|tag| {
            let count = tag_counts.get(tag.as_str()).copied().unwrap_or(0);
            let line = Line::from(vec![
                Span::styled(format!(" #{}", tag), theme::bold()),
                Span::styled(format!(" ({})", count), theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Filter by Tag ", theme::brand()))
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, chunks[0], &mut app.ui.tag_picker_state);

    let spans = vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" select ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" back", theme::muted()),
    ];
    super::render_footer_with_status(frame, chunks[1], spans, app);
}
