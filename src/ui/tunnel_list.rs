use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem, Paragraph};

use super::design;
use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App, alias: &str) {
    let is_active = app.active_tunnels.contains_key(alias);
    let is_readonly = app
        .hosts
        .iter()
        .any(|h| h.alias == alias && h.source_file.is_some());

    // Overlay: percentage-based width, height fits content
    let item_count = app.tunnel_list.len().max(1);
    let height = (item_count as u16 + 6).min(frame.area().height.saturating_sub(4));
    let area = design::overlay_area(frame, 70, 80, height);
    frame.render_widget(Clear, area);

    let mut block = design::overlay_block(&format!("Tunnels for {}", alias));
    if is_active {
        block = block.title_top(Line::from(Span::styled("[running] ", theme::success())));
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (content, footer) = design::content_and_footer(inner);

    if app.tunnel_list.is_empty() {
        let msg = if is_readonly {
            "  Read-only (included file)."
        } else {
            "  No tunnels. Press 'a' to add one."
        };
        frame.render_widget(Paragraph::new(msg).style(theme::muted()), content);
    } else {
        let items: Vec<ListItem> = app
            .tunnel_list
            .iter()
            .map(|rule| {
                let type_label = format!(" {:<10}", rule.tunnel_type.label());
                let port_str = if rule.bind_address.is_empty() {
                    format!("{}", rule.bind_port)
                } else if rule.bind_address.contains(':') {
                    format!("[{}]:{}", rule.bind_address, rule.bind_port)
                } else {
                    format!("{}:{}", rule.bind_address, rule.bind_port)
                };
                let dest = match rule.tunnel_type {
                    crate::tunnel::TunnelType::Dynamic => "(SOCKS proxy)".to_string(),
                    _ => {
                        if rule.remote_host.contains(':') {
                            format!("[{}]:{}", rule.remote_host, rule.remote_port)
                        } else {
                            format!("{}:{}", rule.remote_host, rule.remote_port)
                        }
                    }
                };
                let line = Line::from(vec![
                    Span::styled(type_label, theme::bold()),
                    Span::styled(format!("{:<14}", port_str), theme::bold()),
                    Span::raw("  "),
                    Span::styled(dest, theme::muted()),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .highlight_style(theme::selected_row())
            .highlight_symbol(design::LIST_HIGHLIGHT);

        frame.render_stateful_widget(list, content, &mut app.ui.tunnel_list_state);
    }

    // Footer
    if app.pending_tunnel_delete.is_some() {
        let mut spans = vec![Span::styled(" Remove tunnel? ", theme::bold())];
        spans.extend(
            design::Footer::new()
                .action("y", " yes ")
                .action("Esc", " no")
                .into_spans(),
        );
        super::render_footer_with_status(frame, footer, spans, app);
    } else {
        let mut f = design::Footer::new();
        if is_active {
            f = f.primary("Enter", " stop ");
        } else if !app.tunnel_list.is_empty() {
            f = f.primary("Enter", " start ");
        }
        if !is_readonly {
            f = f.action("a", " add ");
            if !app.tunnel_list.is_empty() {
                f = f.action("e", " edit ").action("d", " del ");
            }
        }
        f = f.action("Esc", " back");
        f.render_with_status(frame, footer, app);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::{Constraint, Layout, Rect};

    #[test]
    fn layout_has_spacer_between_content_and_footer() {
        let area = Rect::new(0, 0, 60, 20);
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        // chunks[0] = content, chunks[1] = spacer, chunks[2] = footer
        assert_eq!(chunks[1].height, 1, "spacer row should be 1 tall");
        assert_eq!(chunks[2].height, 1, "footer row should be 1 tall");
        assert!(
            chunks[2].y > chunks[0].y + chunks[0].height,
            "footer (y={}) should be below content end (y={})",
            chunks[2].y,
            chunks[0].y + chunks[0].height
        );
    }
}
