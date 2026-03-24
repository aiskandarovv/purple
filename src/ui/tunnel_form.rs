use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, Screen, TunnelFormField};
use crate::tunnel::TunnelType;

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let title = match &app.screen {
        Screen::TunnelForm {
            editing: Some(_), ..
        } => " Edit Tunnel ",
        _ => " Add Tunnel ",
    };

    let is_dynamic = app.tunnel_form.tunnel_type == TunnelType::Dynamic;

    let fields: Vec<TunnelFormField> = if is_dynamic {
        vec![TunnelFormField::Type, TunnelFormField::BindPort]
    } else {
        vec![
            TunnelFormField::Type,
            TunnelFormField::BindPort,
            TunnelFormField::RemoteHost,
            TunnelFormField::RemotePort,
        ]
    };

    // Block: top(1) + fields * 2 (divider + content) + bottom(1)
    let block_height = 2 + fields.len() as u16 * 2;
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

    for (i, &field) in fields.iter().enumerate() {
        let divider_y = inner.y + (2 * i) as u16;
        let content_y = divider_y + 1;

        let is_focused = app.tunnel_form.focused_field == field;
        let label_style = if is_focused {
            theme::accent_bold()
        } else {
            theme::muted()
        };
        let label = format!(" {}* ", field.label());
        render_divider(
            frame,
            block_area,
            divider_y,
            &label,
            label_style,
            theme::border(),
        );

        let content_area = Rect::new(inner.x + 1, content_y, inner.width.saturating_sub(1), 1);
        render_field_content(frame, content_area, field, &app.tunnel_form);
    }

    // Footer below the block
    let footer_area = Rect::new(form_area.x, form_area.y + block_height, form_area.width, 1);
    let footer_spans = if app.pending_discard_confirm {
        vec![
            Span::styled(" Discard changes? ", theme::error()),
            Span::styled("y", theme::accent_bold()),
            Span::styled(" yes ", theme::muted()),
            Span::styled("\u{2502} ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" no", theme::muted()),
        ]
    } else {
        vec![
            Span::styled(" Enter", theme::primary_action()),
            Span::styled(" save ", theme::muted()),
            Span::styled("\u{2502} ", theme::muted()),
            Span::styled("Tab", theme::accent_bold()),
            Span::styled(" next ", theme::muted()),
            Span::styled("Space", theme::accent_bold()),
            Span::styled(" type ", theme::muted()),
            Span::styled("\u{2502} ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" cancel", theme::muted()),
        ]
    };
    super::render_footer_with_status(frame, footer_area, footer_spans, app);
}

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

fn render_field_content(
    frame: &mut Frame,
    area: Rect,
    field: TunnelFormField,
    form: &crate::app::TunnelForm,
) {
    let is_focused = form.focused_field == field;

    if field == TunnelFormField::Type {
        let type_label = form.tunnel_type.label();
        let content = if is_focused {
            let inner_width = area.width as usize;
            let val_width = type_label.len();
            let gap = inner_width.saturating_sub(val_width + 3);
            Line::from(vec![
                Span::styled(type_label, theme::bold()),
                Span::raw(" ".repeat(gap)),
                Span::styled("\u{2423}", theme::muted()),
            ])
        } else {
            Line::from(Span::styled(type_label, theme::bold()))
        };
        frame.render_widget(Paragraph::new(content), area);
        return;
    }

    let value = match field {
        TunnelFormField::BindPort => &form.bind_port,
        TunnelFormField::RemoteHost => &form.remote_host,
        TunnelFormField::RemotePort => &form.remote_port,
        TunnelFormField::Type => unreachable!(),
    };

    let placeholder = match field {
        TunnelFormField::BindPort => "8080",
        TunnelFormField::RemoteHost => "localhost",
        TunnelFormField::RemotePort => "80",
        TunnelFormField::Type => unreachable!(),
    };

    let content = if value.is_empty() && is_focused {
        Line::from(Span::styled(placeholder, theme::muted()))
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
