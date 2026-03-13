use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, FormField, Screen};

fn placeholder_for(field: FormField) -> String {
    match field {
        FormField::AskPass => {
            if let Some(default) = crate::preferences::load_askpass_default() {
                format!("default: {}", default)
            } else {
                "Enter to pick a source".to_string()
            }
        }
        FormField::Alias => "my-server".to_string(),
        FormField::Hostname => "192.168.1.1 or example.com".to_string(),
        FormField::User => "root".to_string(),
        FormField::Port => "22".to_string(),
        FormField::IdentityFile => "Enter to pick a key".to_string(),
        FormField::ProxyJump => "Enter to pick a host".to_string(),
        FormField::Tags => "prod, staging, us-east".to_string(),
    }
}

/// All form fields in display order with required flag.
const FIELDS: &[(FormField, bool)] = &[
    (FormField::Alias, true),
    (FormField::Hostname, true),
    (FormField::User, false),
    (FormField::Port, false),
    (FormField::IdentityFile, false),
    (FormField::ProxyJump, false),
    (FormField::AskPass, false),
    (FormField::Tags, false),
];

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let title = match &app.screen {
        Screen::AddHost => " Add New Host ",
        Screen::EditHost { .. } => " Edit Host ",
        _ => " Host ",
    };

    // Block: top(1) + fields * 2 (divider + content) + bottom(1)
    let block_height = 2 + FIELDS.len() as u16 * 2;
    let total_height = block_height + 1; // + footer

    let base = super::centered_rect(70, 80, area);
    let form_area = super::centered_rect_fixed(base.width, total_height, area);
    frame.render_widget(Clear, form_area);

    let block_area = Rect::new(form_area.x, form_area.y, form_area.width, block_height);

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(title, theme::brand()))
        .border_style(theme::border());

    let inner = block.inner(block_area);
    frame.render_widget(block, block_area);

    // Render dividers and content for each field
    for (i, &(field, is_required)) in FIELDS.iter().enumerate() {
        let divider_y = inner.y + (2 * i) as u16;
        let content_y = divider_y + 1;

        let is_focused = app.form.focused_field == field;
        let label_style = if is_focused { theme::accent_bold() } else { theme::muted() };
        let label = if is_required {
            format!(" {}* ", field.label())
        } else {
            format!(" {} ", field.label())
        };
        render_divider(frame, block_area, divider_y, &label, label_style, theme::border());

        let content_area = Rect::new(inner.x + 1, content_y, inner.width.saturating_sub(1), 1);
        render_field_content(frame, content_area, field, &app.form);
    }

    // Footer below the block
    let footer_area = Rect::new(form_area.x, form_area.y + block_height, form_area.width, 1);
    let mut footer_spans = vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" save ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Tab", theme::accent_bold()),
        Span::styled(" next ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" cancel", theme::muted()),
    ];
    if let Some(ref hint) = app.form.form_hint {
        let hint_width: usize = hint.width() + 4; // " ⚠ {hint} "
        let shortcuts_width: usize = footer_spans.iter().map(|s| s.width()).sum();
        let total = footer_area.width as usize;
        let gap = total.saturating_sub(shortcuts_width + hint_width);
        if gap > 0 {
            footer_spans.push(Span::raw(" ".repeat(gap)));
            footer_spans.push(Span::styled(format!("\u{26A0} {} ", hint), theme::error()));
        }
    }
    // Only use render_footer_with_status when no form_hint (to avoid double status)
    if app.form.form_hint.is_some() {
        frame.render_widget(Paragraph::new(Line::from(footer_spans)), footer_area);
    } else {
        super::render_footer_with_status(frame, footer_area, footer_spans, app);
    }

    // Key picker popup overlay
    if app.ui.show_key_picker {
        render_key_picker_overlay(frame, app);
    }

    // ProxyJump picker popup overlay
    if app.ui.show_proxyjump_picker {
        render_proxyjump_picker_overlay(frame, app);
    }

    // Password source picker popup overlay
    if app.ui.show_password_picker {
        render_password_picker_overlay(frame, app);
    }
}

/// Render the key picker popup overlay. Public for reuse from provider form.
pub fn render_key_picker_overlay(frame: &mut Frame, app: &mut App) {
    if app.keys.is_empty() {
        // Small popup saying no keys found
        let area = super::centered_rect_fixed(50, 5, frame.area());
        frame.render_widget(Clear, area);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Span::styled(" Select Key ", theme::brand()))
            .border_style(theme::accent());
        let msg = Paragraph::new(Line::from(Span::styled(
            "  No keys found in ~/.ssh/",
            theme::muted(),
        )))
        .block(block);
        frame.render_widget(msg, area);
        return;
    }

    let height = (app.keys.len() as u16 + 4).min(16);
    let width = frame.area().width.clamp(58, 72);
    let area = super::centered_rect_fixed(width, height, frame.area());
    frame.render_widget(Clear, area);

    // Comment gets remaining space after name(16) + type(10) + borders(2) + highlight(2) + lead(1)
    let comment_max = (width as usize).saturating_sub(2 + 2 + 1 + 16 + 10);

    let items: Vec<ListItem> = app
        .keys
        .iter()
        .map(|key| {
            let type_display = key.type_display();
            let comment = if key.comment.is_empty() {
                String::new()
            } else {
                super::truncate(&key.comment, comment_max)
            };
            let line = Line::from(vec![
                Span::styled(format!(" {:<16}", key.name), theme::bold()),
                Span::styled(format!("{:<10}", type_display), theme::muted()),
                Span::styled(comment, theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Select Key ", theme::brand()))
        .border_style(theme::accent());

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_row())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, area, &mut app.ui.key_picker_state);
}

fn render_proxyjump_picker_overlay(frame: &mut Frame, app: &mut App) {
    let candidates = app.proxyjump_candidates();

    if candidates.is_empty() {
        let area = super::centered_rect_fixed(50, 5, frame.area());
        frame.render_widget(Clear, area);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Span::styled(" ProxyJump ", theme::brand()))
            .border_style(theme::accent());
        let msg = Paragraph::new(Line::from(Span::styled(
            "  No other hosts configured",
            theme::muted(),
        )))
        .block(block);
        frame.render_widget(msg, area);
        return;
    }

    let height = (candidates.len() as u16 + 2).min(16);
    let width = frame.area().width.clamp(50, 64);
    let area = super::centered_rect_fixed(width, height, frame.area());
    frame.render_widget(Clear, area);

    let host_max = (width as usize).saturating_sub(2 + 2 + 1 + 20);

    let items: Vec<ListItem> = candidates
        .iter()
        .map(|(alias, hostname)| {
            let host_display = super::truncate(hostname, host_max);
            let line = Line::from(vec![
                Span::styled(format!(" {:<20}", super::truncate(alias, 20)), theme::bold()),
                Span::styled(host_display, theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" ProxyJump ", theme::brand()))
        .border_style(theme::accent());

    let list = List::new(items)
        .block(block)
        .highlight_style(theme::selected_row())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, area, &mut app.ui.proxyjump_picker_state);
}

fn render_password_picker_overlay(frame: &mut Frame, app: &mut App) {
    let sources = crate::askpass::PASSWORD_SOURCES;
    let height = sources.len() as u16 + 4; // items + borders + footer
    let area = super::centered_rect_fixed(54, height, frame.area());
    frame.render_widget(Clear, area);

    let items: Vec<ListItem> = sources
        .iter()
        .map(|src| {
            let hint_width = src.hint.len();
            let label_width = 48_usize.saturating_sub(4).saturating_sub(hint_width).saturating_sub(1);
            let line = Line::from(vec![
                Span::styled(format!(" {:<width$}", src.label, width = label_width), theme::bold()),
                Span::styled(src.hint, theme::muted()),
            ]);
            ListItem::new(line)
        })
        .collect();

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Password Source ", theme::brand()))
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split into list area and footer
    let chunks = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Min(1),
        ratatui::layout::Constraint::Length(1),
    ])
    .split(inner);

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol("  ");

    frame.render_stateful_widget(list, chunks[0], &mut app.ui.password_picker_state);

    let spans = vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" select ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Ctrl+D", theme::accent_bold()),
        Span::styled(" global default ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" cancel", theme::muted()),
    ];
    super::render_footer_with_status(frame, chunks[1], spans, app);
}

/// Get the placeholder text for a field (public for tests).
#[cfg(test)]
pub fn placeholder_text(field: FormField) -> String {
    placeholder_for(field)
}

/// Delegate to shared render_divider in mod.rs.
fn render_divider(
    frame: &mut Frame,
    block_area: Rect,
    y: u16,
    label: &str,
    label_style: Style,
    border_style: Style,
) {
    super::render_divider(frame, block_area, y, label, label_style, border_style);
}

/// Render a single field's content (value or placeholder) and set cursor.
fn render_field_content(
    frame: &mut Frame,
    area: Rect,
    field: FormField,
    form: &crate::app::HostForm,
) {
    let is_focused = form.focused_field == field;

    let value = match field {
        FormField::Alias => &form.alias,
        FormField::Hostname => &form.hostname,
        FormField::User => &form.user,
        FormField::Port => &form.port,
        FormField::IdentityFile => &form.identity_file,
        FormField::ProxyJump => &form.proxy_jump,
        FormField::AskPass => &form.askpass,
        FormField::Tags => &form.tags,
    };

    let is_picker = matches!(field, FormField::IdentityFile | FormField::ProxyJump | FormField::AskPass);

    // Show placeholder only when field is empty and focused
    let content = if value.is_empty() && is_focused && !is_picker {
        let ph = placeholder_for(field);
        Line::from(Span::styled(ph, theme::muted()))
    } else if is_picker && is_focused {
        let inner_width = area.width as usize;
        let arrow_pos = inner_width.saturating_sub(1);
        let (display, display_style) = if value.is_empty() {
            (placeholder_for(field), theme::muted())
        } else {
            (value.to_string(), theme::bold())
        };
        let val_width = display.width();
        let gap = arrow_pos.saturating_sub(val_width);
        Line::from(vec![
            Span::styled(display, display_style),
            Span::raw(" ".repeat(gap)),
            Span::styled("\u{25B8}", theme::muted()),
        ])
    } else if value.is_empty() {
        Line::from(Span::raw(""))
    } else {
        Line::from(Span::styled(value.to_string(), theme::bold()))
    };

    frame.render_widget(Paragraph::new(content), area);

    if is_focused {
        let prefix: String = value.chars().take(form.cursor_pos).collect();
        let cursor_x = area
            .x
            .saturating_add(prefix.width().min(u16::MAX as usize) as u16);
        let cursor_y = area.y;
        if area.width > 0 && cursor_x < area.x.saturating_add(area.width) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}
