mod confirm_dialog;
mod detail_panel;
mod file_browser;
mod help;
mod host_detail;
pub mod host_form;
mod host_list;
mod key_detail;
mod key_list;
mod provider_list;
mod snippet_form;
mod snippet_picker;
mod tag_picker;
pub mod theme;
mod tunnel_form;
mod tunnel_list;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Screen};

const MIN_WIDTH: u16 = 50;
const MIN_HEIGHT: u16 = 10;

/// Top-level render dispatcher.
pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    // Terminal too small guard
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = Paragraph::new(Line::from(vec![
            Span::styled("\u{26A0}", theme::error()),
            Span::raw(" Terminal too small. Need at least 50x10."),
        ]));
        frame.render_widget(msg, area);
        return;
    }

    match &app.screen {
        Screen::HostList => host_list::render(frame, app),
        Screen::AddHost | Screen::EditHost { .. } => {
            render_host_list_without_status(frame, app);
            host_form::render(frame, app);
        }
        Screen::ConfirmDelete { alias } => {
            let alias = alias.clone();
            render_host_list_without_status(frame, app);
            confirm_dialog::render(frame, app, &alias);
        }
        Screen::Help => {
            render_host_list_without_status(frame, app);
            help::render(frame, app);
        }
        Screen::KeyList => {
            render_host_list_without_status(frame, app);
            key_list::render(frame, app);
        }
        Screen::KeyDetail { index } => {
            let index = *index;
            render_host_list_without_status(frame, app);
            key_list::render(frame, app);
            key_detail::render(frame, app, index);
        }
        Screen::HostDetail { index } => {
            let index = *index;
            render_host_list_without_status(frame, app);
            host_detail::render(frame, app, index);
        }
        Screen::TagPicker => {
            render_host_list_without_status(frame, app);
            tag_picker::render(frame, app);
        }
        Screen::Providers => {
            render_host_list_without_status(frame, app);
            provider_list::render_provider_list(frame, app);
        }
        Screen::ProviderForm { provider } => {
            let provider = provider.clone();
            render_host_list_without_status(frame, app);
            provider_list::render_provider_form(frame, app, &provider);
        }
        Screen::TunnelList { alias } => {
            let alias = alias.clone();
            render_host_list_without_status(frame, app);
            tunnel_list::render(frame, app, &alias);
        }
        Screen::TunnelForm { alias, .. } => {
            let alias = alias.clone();
            render_host_list_without_status(frame, app);
            tunnel_list::render(frame, app, &alias);
            tunnel_form::render(frame, app);
        }
        Screen::SnippetPicker { .. } => {
            render_host_list_without_status(frame, app);
            snippet_picker::render(frame, app);
        }
        Screen::SnippetForm { .. } => {
            render_host_list_without_status(frame, app);
            snippet_picker::render(frame, app);
            snippet_form::render(frame, app);
        }
        Screen::ConfirmHostKeyReset { hostname, .. } => {
            let hostname = hostname.clone();
            render_host_list_without_status(frame, app);
            confirm_dialog::render_host_key_reset(frame, app, &hostname);
        }
        Screen::FileBrowser { .. } => {
            render_host_list_without_status(frame, app);
            file_browser::render(frame, app);
        }
    }
}

/// Render the host list background without status message in the footer.
fn render_host_list_without_status(frame: &mut Frame, app: &mut App) {
    let status = app.status.take();
    host_list::render(frame, app);
    app.status = status;
}

/// Render footer with shortcuts always visible and optional status right-aligned.
pub fn render_footer_with_status(
    frame: &mut Frame,
    area: Rect,
    mut footer_spans: Vec<Span<'_>>,
    app: &App,
) {
    if let Some(ref status) = app.status {
        use unicode_width::UnicodeWidthStr;
        let shortcuts_width: usize = footer_spans.iter().map(|s| s.width()).sum();
        let total_width = area.width as usize;
        let (icon, icon_style, text) = if status.is_error {
            ("\u{26A0}", theme::error(), format!(" {} ", status.text))
        } else {
            ("\u{2713} ", theme::success(), format!("{} ", status.text))
        };
        let status_width = icon.width() + text.width();
        let gap = total_width.saturating_sub(shortcuts_width + status_width);
        if gap > 0 {
            footer_spans.push(Span::raw(" ".repeat(gap)));
            footer_spans.push(Span::styled(icon, icon_style));
            footer_spans.push(Span::raw(text));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(footer_spans)), area);
}

/// Create a centered rect of given percentage within the parent rect.
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

/// Truncate a string to fit within `max_cols` display columns (unicode-width-aware).
pub(crate) fn truncate(s: &str, max_cols: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if s.width() <= max_cols {
        return s.to_string();
    }
    if max_cols <= 1 {
        return String::new();
    }
    let target = max_cols - 1;
    let mut col = 0;
    let mut byte_end = 0;
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col + w > target {
            break;
        }
        col += w;
        byte_end += ch.len_utf8();
    }
    format!("{}…", &s[..byte_end])
}

/// Render a horizontal divider: ├─ Label ───────┤
pub(crate) fn render_divider(
    frame: &mut Frame,
    block_area: Rect,
    y: u16,
    label: &str,
    label_style: Style,
    border_style: Style,
) {
    let width = block_area.width as usize;
    let label_w = label.width();
    let fill = width.saturating_sub(3 + label_w);
    let line = Line::from(vec![
        Span::styled("├─", border_style),
        Span::styled(label.to_string(), label_style),
        Span::styled(format!("{}┤", "─".repeat(fill)), border_style),
    ]);
    frame.render_widget(
        Paragraph::new(line),
        Rect::new(block_area.x, y, block_area.width, 1),
    );
}

/// Create a centered rect with fixed dimensions.
pub fn centered_rect_fixed(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
