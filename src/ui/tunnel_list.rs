use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App, alias: &str) {
    let area = frame.area();
    let is_active = app.active_tunnels.contains_key(alias);
    let is_readonly = app
        .hosts
        .iter()
        .any(|h| h.alias == alias && h.source_file.is_some());

    // Title
    let mut title_spans = vec![
        Span::styled(" purple. ", theme::brand_badge()),
        Span::raw(format!(" Tunnels for {} ", alias)),
    ];
    if is_active {
        title_spans.push(Span::styled("[running] ", theme::success()));
    }
    let title = Line::from(title_spans);

    let chunks = Layout::vertical([
        Constraint::Min(5),    // Tunnel list
        Constraint::Length(1), // Footer or status
    ])
    .split(area);

    if app.tunnel_list.is_empty() {
        let msg = if is_readonly {
            "  No tunnels configured. This host is read-only (included file)."
        } else {
            "  No tunnels configured. Press 'a' to add one."
        };
        let empty_msg = Paragraph::new(msg)
            .style(theme::muted())
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(theme::border()),
            );
        frame.render_widget(empty_msg, chunks[0]);
    } else {
        let items: Vec<ListItem> = app
            .tunnel_list
            .iter()
            .map(|rule| {
                let type_label = format!("  {:<10}", rule.tunnel_type.label());
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
                    Span::styled(format!("{:<8}", port_str), theme::bold()),
                    Span::raw("  "),
                    Span::styled(dest, theme::muted()),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(theme::border()),
            )
            .highlight_style(theme::selected())
            .highlight_symbol("  ");

        frame.render_stateful_widget(list, chunks[0], &mut app.ui.tunnel_list_state);
    }

    // Footer or status
    if app.status.is_some() {
        super::render_status_bar(frame, chunks[1], app);
    } else {
        let mut spans = Vec::new();
        if is_active {
            spans.push(Span::styled(" Enter", theme::primary_action()));
            spans.push(Span::styled(" stop  ", theme::muted()));
        } else if !app.tunnel_list.is_empty() {
            spans.push(Span::styled(" Enter", theme::primary_action()));
            spans.push(Span::styled(" start  ", theme::muted()));
        }
        if !is_readonly {
            spans.push(Span::styled("a", theme::accent_bold()));
            spans.push(Span::styled(" add  ", theme::muted()));
            if !app.tunnel_list.is_empty() {
                spans.push(Span::styled("e", theme::accent_bold()));
                spans.push(Span::styled(" edit  ", theme::muted()));
                spans.push(Span::styled("d", theme::accent_bold()));
                spans.push(Span::styled(" delete  ", theme::muted()));
            }
        }
        spans.push(Span::styled("Esc", theme::accent_bold()));
        spans.push(Span::styled(" back", theme::muted()));
        let footer = Line::from(spans);
        frame.render_widget(Paragraph::new(footer), chunks[1]);
    }
}
