use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, List, ListItem};

use super::design;
use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App, alias: &str) {
    let is_active = app.active_tunnels.contains_key(alias);
    let is_readonly = app
        .hosts
        .iter()
        .any(|h| h.alias == alias && h.source_file.is_some());

    // Overlay: percentage-based width, height fits content. Reserve 1 row
    // below the block for the external footer.
    let item_count = app.tunnel_list.len().max(1);
    let height = (item_count as u16 + 4).min(frame.area().height.saturating_sub(5));
    let area = design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, height);
    frame.render_widget(Clear, area);

    let mut block = design::overlay_block(&format!("Tunnels for {}", alias));
    if is_active {
        block = block.title_top(Line::from(Span::styled("[running] ", theme::success())));
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.tunnel_list.is_empty() {
        if is_readonly {
            design::render_empty(frame, inner, "Read-only (included file).");
        } else {
            design::render_empty_with_hint(frame, inner, "No tunnels.", "a", "add one");
        }
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

        frame.render_stateful_widget(list, inner, &mut app.ui.tunnel_list_state);
    }

    // Footer below the block
    let footer_area = design::render_overlay_footer(frame, area);
    if app.pending_tunnel_delete.is_some() {
        let mut spans = vec![Span::styled(" Remove tunnel? ", theme::bold())];
        spans.extend(
            design::Footer::new()
                .action("y", " yes ")
                .action("Esc", " no")
                .into_spans(),
        );
        super::render_footer_with_status(frame, footer_area, spans, app);
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
        f.render_with_status(frame, footer_area, app);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::design;

    #[test]
    fn footer_sits_directly_below_block() {
        let area = Rect::new(0, 0, 60, 20);
        let footer = design::form_footer(area, area.height);
        assert_eq!(footer.height, 1);
        assert_eq!(footer.y, area.y + area.height);
        assert_eq!(footer.x, area.x);
        assert_eq!(footer.width, area.width);
    }
}
