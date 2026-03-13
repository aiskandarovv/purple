use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph};

use super::theme;
use crate::app::{App, Screen};

pub fn render(frame: &mut Frame, app: &mut App) {
    let host_count = match &app.screen {
        Screen::SnippetPicker { target_aliases } => target_aliases.len(),
        Screen::SnippetForm { target_aliases, .. } => target_aliases.len(),
        _ => 1,
    };

    let title = if host_count > 1 {
        Line::from(vec![
            Span::styled(format!(" Snippets ({} hosts) ", host_count), theme::brand()),
        ])
    } else {
        Line::from(vec![
            Span::styled(" Snippets ", theme::brand()),
        ])
    };

    let item_count = app.snippet_store.snippets.len().max(1);
    let height = (item_count as u16 + 5).min(frame.area().height.saturating_sub(4));
    let area = {
        let r = super::centered_rect(70, 80, frame.area());
        super::centered_rect_fixed(r.width, height, frame.area())
    };
    frame.render_widget(Clear, area);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);

    if app.snippet_store.snippets.is_empty() {
        frame.render_widget(
            Paragraph::new("  No snippets yet. Press 'a' to add one.").style(theme::muted()),
            chunks[0],
        );
    } else {
        let items: Vec<ListItem> = app
            .snippet_store
            .snippets
            .iter()
            .map(|snippet| {
                let mut spans = vec![
                    Span::styled(format!(" {:<20}", super::truncate(&snippet.name, 20)), theme::bold()),
                    Span::styled(super::truncate(&snippet.command, 30), theme::muted()),
                ];
                if !snippet.description.is_empty() {
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        super::truncate(&snippet.description, 20),
                        theme::muted(),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .highlight_style(theme::selected_row())
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, chunks[0], &mut app.ui.snippet_picker_state);
    }

    // Footer
    if app.pending_snippet_delete.is_some() {
        let name = app.pending_snippet_delete
            .and_then(|i| app.snippet_store.snippets.get(i))
            .map(|s| s.name.as_str())
            .unwrap_or("");
        super::render_footer_with_status(frame, chunks[1], vec![
            Span::styled(format!(" Remove '{}'? ", super::truncate(name, 20)), theme::bold()),
            Span::styled("y", theme::accent_bold()),
            Span::styled(" yes ", theme::muted()),
            Span::styled("\u{2502} ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" no", theme::muted()),
        ], app);
    } else {
        let mut spans: Vec<Span<'_>> = Vec::new();
        if !app.snippet_store.snippets.is_empty() {
            spans.push(Span::styled(" Enter", theme::primary_action()));
            spans.push(Span::styled(" run ", theme::muted()));
            spans.push(Span::styled("\u{2502} ", theme::muted()));
        }
        spans.push(Span::styled("a", theme::accent_bold()));
        spans.push(Span::styled(" add ", theme::muted()));
        if !app.snippet_store.snippets.is_empty() {
            spans.push(Span::styled("e", theme::accent_bold()));
            spans.push(Span::styled(" edit ", theme::muted()));
            spans.push(Span::styled("d", theme::accent_bold()));
            spans.push(Span::styled(" delete ", theme::muted()));
        }
        spans.push(Span::styled("\u{2502} ", theme::muted()));
        spans.push(Span::styled("Esc", theme::accent_bold()));
        spans.push(Span::styled(" back", theme::muted()));
        super::render_footer_with_status(frame, chunks[1], spans, app);
    }
}
