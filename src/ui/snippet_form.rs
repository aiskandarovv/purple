use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::design;
use super::theme;
use crate::app::{App, Screen, SnippetFormField};

pub fn render(frame: &mut Frame, app: &mut App) {
    let title = match &app.screen {
        Screen::SnippetForm {
            editing: Some(_), ..
        } => "Snippets > Edit",
        _ => "Snippets > Add",
    };

    let fields = SnippetFormField::ALL;

    // Block: top(1) + fields * 2 (divider + content) + bottom(1)
    let block_height = 2 + fields.len() as u16 * 2;
    let total_height = block_height + 1; // + footer

    let form_area = design::overlay_area(frame, design::OVERLAY_W, design::OVERLAY_H, total_height);
    frame.render_widget(Clear, form_area);

    let block_area = Rect::new(form_area.x, form_area.y, form_area.width, block_height);

    let block = design::overlay_block(title);
    let inner = block.inner(block_area);
    frame.render_widget(block, block_area);

    for (i, &field) in fields.iter().enumerate() {
        let divider_y = design::form_divider_y(inner, i);
        let content_y = divider_y + 1;

        let is_focused = app.snippet_form.focused_field == field;
        let label_style = if is_focused {
            theme::accent_bold()
        } else {
            theme::muted()
        };
        let required = matches!(field, SnippetFormField::Name | SnippetFormField::Command);
        let label = if required {
            format!(" {}* ", field.label())
        } else {
            format!(" {} ", field.label())
        };
        super::render_divider(
            frame,
            block_area,
            divider_y,
            &label,
            label_style,
            theme::accent(),
        );

        let content_area = Rect::new(inner.x + 1, content_y, inner.width.saturating_sub(1), 1);
        render_field_content(frame, content_area, field, &app.snippet_form);
    }

    // Footer below the block. Snippet form has only text fields, so the
    // dynamic save footer maps to FieldKind::Text (no Space hint).
    let footer_area = design::render_overlay_footer(frame, block_area);
    if app.pending_discard_confirm {
        design::render_discard_prompt(frame, footer_area, app);
    } else {
        design::form_save_footer(design::FormFooterMode::Expanded(design::FieldKind::Text))
            .render_with_status(frame, footer_area, app);
    }
}

fn render_field_content(
    frame: &mut Frame,
    area: Rect,
    field: SnippetFormField,
    form: &crate::app::SnippetForm,
) {
    let is_focused = form.focused_field == field;

    let placeholder = match field {
        SnippetFormField::Name => crate::messages::hints::SNIPPET_NAME,
        SnippetFormField::Command => crate::messages::hints::SNIPPET_COMMAND,
        SnippetFormField::Description => "",
    };

    let field_value = match field {
        SnippetFormField::Name => &form.name,
        SnippetFormField::Command => &form.command,
        SnippetFormField::Description => &form.description,
    };

    let content = if field_value.is_empty() && is_focused {
        if placeholder.is_empty() {
            Line::from(Span::styled(
                crate::messages::hints::SNIPPET_OPTIONAL,
                theme::muted(),
            ))
        } else {
            Line::from(Span::styled(placeholder, theme::muted()))
        }
    } else if field_value.is_empty() {
        Line::from(Span::raw(""))
    } else {
        Line::from(Span::styled(field_value.to_string(), theme::bold()))
    };

    frame.render_widget(Paragraph::new(content), area);

    if is_focused {
        let prefix: String = field_value.chars().take(form.cursor_pos).collect();
        let cursor_x = area
            .x
            .saturating_add(prefix.width().min(u16::MAX as usize) as u16);
        let cursor_y = area.y;
        if area.width > 0 && cursor_x < area.x.saturating_add(area.width) {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}
