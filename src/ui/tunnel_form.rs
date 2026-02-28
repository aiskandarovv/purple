use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, Screen, TunnelFormField};
use crate::tunnel::TunnelType;

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let title = match &app.screen {
        Screen::TunnelForm { editing: Some(_), .. } => " Edit Tunnel ",
        _ => " Add Tunnel ",
    };

    let is_dynamic = app.tunnel_form.tunnel_type == TunnelType::Dynamic;

    // Fixed-size overlay
    let height: u16 = if is_dynamic { 10 } else { 15 };
    let form_area = super::centered_rect_fixed(50, height, area);

    frame.render_widget(Clear, form_area);

    let outer_block = Block::default()
        .title(Span::styled(title, theme::brand()))
        .borders(Borders::ALL)
        .border_style(theme::border());

    let inner = outer_block.inner(form_area);
    frame.render_widget(outer_block, form_area);

    let mut constraints = vec![
        Constraint::Length(3), // Type
        Constraint::Length(3), // Bind Port
    ];
    if !is_dynamic {
        constraints.push(Constraint::Length(3)); // Remote Host
        constraints.push(Constraint::Length(3)); // Remote Port
    }
    constraints.push(Constraint::Min(0));   // Spacer
    constraints.push(Constraint::Length(1)); // Footer

    let chunks = Layout::vertical(constraints).split(inner);

    // Type field (special: Left/Right cycle, not text input)
    render_type_field(frame, chunks[0], &app.tunnel_form);

    // Bind Port
    render_text_field(
        frame,
        chunks[1],
        TunnelFormField::BindPort,
        &app.tunnel_form.bind_port,
        app.tunnel_form.focused_field,
        "8080",
        true,
    );

    if !is_dynamic {
        // Remote Host
        render_text_field(
            frame,
            chunks[2],
            TunnelFormField::RemoteHost,
            &app.tunnel_form.remote_host,
            app.tunnel_form.focused_field,
            "localhost",
            true,
        );

        // Remote Port
        render_text_field(
            frame,
            chunks[3],
            TunnelFormField::RemotePort,
            &app.tunnel_form.remote_port,
            app.tunnel_form.focused_field,
            "80",
            true,
        );
    }

    // Footer or status
    let footer_idx = chunks.len() - 1;
    if app.status.is_some() {
        super::render_status_bar(frame, chunks[footer_idx], app);
    } else {
        let footer = Line::from(vec![
            Span::styled(" Enter", theme::primary_action()),
            Span::styled(" save  ", theme::muted()),
            Span::styled("Left/Right", theme::accent_bold()),
            Span::styled(" type  ", theme::muted()),
            Span::styled("Tab/S-Tab", theme::accent_bold()),
            Span::styled(" navigate  ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" cancel", theme::muted()),
        ]);
        frame.render_widget(Paragraph::new(footer), chunks[footer_idx]);
    }
}

fn render_type_field(frame: &mut Frame, area: Rect, form: &crate::app::TunnelForm) {
    let is_focused = form.focused_field == TunnelFormField::Type;

    let (border_style, label_style) = if is_focused {
        (theme::border_focused(), theme::accent_bold())
    } else {
        (theme::border(), theme::muted())
    };

    let block = Block::default()
        .title(Span::styled(" Type* ", label_style))
        .borders(Borders::ALL)
        .border_style(border_style);

    let type_display = format!("< {} >", form.tunnel_type.label());
    let style = if is_focused {
        theme::bold()
    } else {
        ratatui::style::Style::default()
    };
    let paragraph = Paragraph::new(Span::styled(type_display, style)).block(block);
    frame.render_widget(paragraph, area);
}

fn render_text_field(
    frame: &mut Frame,
    area: Rect,
    field: TunnelFormField,
    value: &str,
    focused: TunnelFormField,
    placeholder: &str,
    required: bool,
) {
    let is_focused = focused == field;

    let (border_style, label_style) = if is_focused {
        (theme::border_focused(), theme::accent_bold())
    } else {
        (theme::border(), theme::muted())
    };

    let label = if required {
        format!(" {}* ", field.label())
    } else {
        format!(" {} ", field.label())
    };

    let block = Block::default()
        .title(Span::styled(label, label_style))
        .borders(Borders::ALL)
        .border_style(border_style);

    let display: Span = if value.is_empty() && !is_focused {
        Span::styled(placeholder, theme::muted())
    } else {
        Span::raw(value)
    };

    let paragraph = Paragraph::new(display).block(block);
    frame.render_widget(paragraph, area);

    // Cursor
    if is_focused {
        let cursor_x = area
            .x
            .saturating_add(1)
            .saturating_add(value.width().min(u16::MAX as usize) as u16);
        let cursor_y = area.y + 1;
        if area.width > 1 && cursor_x < area.x.saturating_add(area.width).saturating_sub(1) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}
