mod confirm_dialog;
pub(crate) mod containers;
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
mod snippet_output;
mod snippet_param_form;
mod snippet_picker;
mod tag_picker;
pub mod theme;
mod tunnel_form;
mod tunnel_list;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
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

    // Status messages show in the host list footer (including behind overlays),
    // but not in overlay footers. render_overlay hides app.status while the
    // overlay renders so render_footer_with_status calls inside overlays ignore it.
    match &app.screen {
        Screen::HostList => {
            host_list::render(frame, app);
            // Close animation: paint saved overlay buffer with shrinking clip
            render_overlay_close(frame, app);
        }
        Screen::AddHost | Screen::EditHost { .. } => {
            host_list::render(frame, app);
            render_overlay(frame, app, host_form::render);
        }
        Screen::ConfirmDelete { alias } => {
            let alias = alias.clone();
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                confirm_dialog::render(frame, app, &alias)
            });
        }
        Screen::Help { .. } => {
            // Always render host_list as the base layer. Help can be opened from
            // other screens (file browser, snippets, etc.) but rendering the
            // originating screen would require it to be the active Screen variant.
            // The help overlay covers most of the area so the base is barely visible.
            host_list::render(frame, app);
            render_overlay(frame, app, help::render);
        }
        Screen::KeyList => {
            host_list::render(frame, app);
            render_overlay(frame, app, key_list::render);
        }
        Screen::KeyDetail { index } => {
            let index = *index;
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                key_list::render(frame, app);
                key_detail::render(frame, app, index);
            });
        }
        Screen::HostDetail { index } => {
            let index = *index;
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                host_detail::render(frame, app, index)
            });
        }
        Screen::TagPicker => {
            host_list::render(frame, app);
            render_overlay(frame, app, tag_picker::render);
        }
        Screen::GroupTagPicker => {
            host_list::render(frame, app);
            render_overlay(frame, app, tag_picker::render_group_picker);
        }
        Screen::Providers => {
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                provider_list::render_provider_list(frame, app)
            });
        }
        Screen::ProviderForm { provider } => {
            let provider = provider.clone();
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                provider_list::render_provider_form(frame, app, &provider)
            });
        }
        Screen::TunnelList { alias } => {
            let alias = alias.clone();
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                tunnel_list::render(frame, app, &alias)
            });
        }
        Screen::TunnelForm { alias, .. } => {
            let alias = alias.clone();
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                tunnel_list::render(frame, app, &alias);
                tunnel_form::render(frame, app);
            });
        }
        Screen::SnippetPicker { .. } => {
            host_list::render(frame, app);
            render_overlay(frame, app, snippet_picker::render);
        }
        Screen::SnippetForm { .. } => {
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                snippet_picker::render(frame, app);
                snippet_form::render(frame, app);
            });
        }
        Screen::ConfirmHostKeyReset { hostname, .. } => {
            let hostname = hostname.clone();
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                confirm_dialog::render_host_key_reset(frame, app, &hostname)
            });
        }
        Screen::FileBrowser { .. } => {
            host_list::render(frame, app);
            render_overlay(frame, app, file_browser::render);
        }
        Screen::SnippetOutput { .. } => {
            host_list::render(frame, app);
            render_overlay(frame, app, snippet_output::render);
        }
        Screen::SnippetParamForm { .. } => {
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                snippet_picker::render(frame, app);
                snippet_param_form::render(frame, app);
            });
        }
        Screen::ConfirmImport { count } => {
            let count = *count;
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                confirm_dialog::render_confirm_import(frame, app, count)
            });
        }
        Screen::Containers { .. } => {
            host_list::render(frame, app);
            render_overlay(frame, app, containers::render);
        }
        Screen::ConfirmPurgeStale { aliases, provider } => {
            let aliases = aliases.clone();
            let provider = provider.clone();
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                confirm_dialog::render_confirm_purge_stale(frame, app, &aliases, &provider)
            });
        }
        Screen::Welcome {
            has_backup,
            host_count,
            known_hosts_count,
        } => {
            let has_backup = *has_backup;
            let host_count = *host_count;
            let known_hosts_count = *known_hosts_count;
            host_list::render(frame, app);
            render_overlay(frame, app, |frame, app| {
                confirm_dialog::render_welcome(
                    frame,
                    app,
                    has_backup,
                    host_count,
                    known_hosts_count,
                )
            });
        }
    }
}

/// Render an overlay with animation support.
///
/// Hides app.status while rendering and applies vertical clip animation
/// for smooth open transitions. Saves the buffer for close animation.
fn render_overlay(frame: &mut Frame, app: &mut App, f: impl FnOnce(&mut Frame, &mut App)) {
    let status = app.status.take();

    dim_background(frame);

    // Save dimmed host list buffer before overlay renders (needed for open
    // animation clip restore). Captured after dim so the background outside the
    // growing clip stays consistently dimmed during the animation.
    let progress = app.overlay_anim_progress();
    let animating_open = progress.is_some();
    let pre_overlay = if animating_open {
        Some(frame.buffer_mut().clone())
    } else {
        None
    };

    f(frame, app);

    // Save the overlay buffer for close animation only once: the first stable
    // frame after the open animation completes. During the open animation the
    // buffer is partially clipped and not suitable for replay. Re-saving every
    // stable frame would waste ~440KB/frame on large terminals.
    if !animating_open && app.overlay_buffer.is_none() {
        app.overlay_buffer = Some(frame.buffer_mut().clone());
    }

    // Apply opening animation: clip overlay to a growing scaled region
    if let (Some(progress), Some(saved)) = (progress, pre_overlay) {
        if progress < 1.0 {
            apply_scale_clip(frame, &saved, progress);
        }
    }

    app.status = status;
}

/// Dim all cells in the frame buffer so the host list behind an overlay appears muted.
/// On truecolor/ANSI-16 terminals the foreground is replaced with dark grey for a
/// stronger effect. Cells that already have a coloured background (badges, selected
/// row) only receive the DIM modifier so their text stays readable.
fn dim_background(frame: &mut Frame) {
    use ratatui::style::Color;

    let dim_only = Style::default().add_modifier(Modifier::DIM);
    let style = match theme::color_mode() {
        2 => Style::default()
            .fg(Color::Rgb(70, 70, 70))
            .add_modifier(Modifier::DIM),
        1 => Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM),
        _ => dim_only,
    };
    let area = frame.area();
    let buf = frame.buffer_mut();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let has_bg = buf[(x, y)].bg != Color::Reset;
            buf[(x, y)].set_style(if has_bg { dim_only } else { style });
        }
    }
}

/// Render the close animation: paint saved overlay buffer with shrinking scale clip.
fn render_overlay_close(frame: &mut Frame, app: &mut App) {
    // Only run when a close animation is active
    let is_closing = app.overlay_anim.as_ref().is_some_and(|a| !a.opening);
    if !is_closing {
        return;
    }

    let progress = match app.overlay_anim_progress() {
        Some(p) => p,
        None => return, // Animation expired; tick_animations handles cleanup
    };

    if let Some(ref saved) = app.overlay_buffer {
        if progress > 0.0 {
            // Dim the host list so the background stays consistently muted
            // while the overlay shrinks away.
            dim_background(frame);

            let area = frame.area();
            let (left, right, top, bottom) = scale_clip_rect(area, progress);

            for y in top..bottom {
                for x in left..right {
                    if let Some(cell) = saved.cell((x, y)) {
                        frame.buffer_mut()[(x, y)] = cell.clone();
                    }
                }
            }
        }
    }
}

/// Clip the frame buffer to a scaled region centered on screen (zoom effect).
/// Cells outside the clip are restored from `saved` (the pre-overlay host list).
fn apply_scale_clip(frame: &mut Frame, saved: &ratatui::buffer::Buffer, progress: f32) {
    let area = frame.area();
    let (left, right, top, bottom) = scale_clip_rect(area, progress);

    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if y < top || y >= bottom || x < left || x >= right {
                if let Some(cell) = saved.cell((x, y)) {
                    frame.buffer_mut()[(x, y)] = cell.clone();
                }
            }
        }
    }
}

/// Calculate the visible rect for a scale/zoom animation centered on the area.
fn scale_clip_rect(area: Rect, progress: f32) -> (u16, u16, u16, u16) {
    let visible_w = (area.width as f32 * progress).ceil() as u16;
    let visible_h = (area.height as f32 * progress).ceil() as u16;
    let left = area.x + area.width.saturating_sub(visible_w) / 2;
    let right = (left + visible_w).min(area.x + area.width);
    let top = area.y + area.height.saturating_sub(visible_h) / 2;
    let bottom = (top + visible_h).min(area.y + area.height);
    (left, right, top, bottom)
}

/// Build a footer action span: key in accent_bold, label in muted.
/// Use this for consistent footers across all screens.
pub fn footer_action<'a>(key: &'a str, label: &'a str) -> [Span<'a>; 2] {
    [
        Span::styled(key, theme::accent_bold()),
        Span::styled(label, theme::muted()),
    ]
}

/// Build a primary footer action span: key in primary_action, label in muted.
pub fn footer_primary<'a>(key: &'a str, label: &'a str) -> [Span<'a>; 2] {
    [
        Span::styled(key, theme::primary_action()),
        Span::styled(label, theme::muted()),
    ]
}

/// Footer separator: │ in muted.
pub fn footer_sep<'a>() -> Span<'a> {
    Span::styled("\u{2502} ", theme::muted())
}

/// Render footer with shortcuts on the left and "? more" pinned to the right edge.
/// When a status message is active, falls back to `render_footer_with_status` behavior.
pub fn render_footer_with_help(
    frame: &mut Frame,
    area: Rect,
    footer_spans: Vec<Span<'_>>,
    app: &App,
) {
    if app.status.is_some() {
        render_footer_with_status(frame, area, footer_spans, app);
        return;
    }
    let right_spans = vec![
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("?", theme::accent_bold()),
        Span::styled(" more", theme::muted()),
    ];
    let right_width: u16 = right_spans.iter().map(|s| s.width()).sum::<usize>() as u16;
    let [left, right] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(right_width)]).areas(area);
    frame.render_widget(Paragraph::new(Line::from(footer_spans)), left);
    frame.render_widget(Paragraph::new(Line::from(right_spans)), right);
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
/// The `├` and `┤` connectors use the border style so they blend with the outer
/// border. The horizontal `─` fill is rendered DIM to keep dividers visually
/// subordinate to the border.
pub(crate) fn render_divider(
    frame: &mut Frame,
    block_area: Rect,
    y: u16,
    label: &str,
    label_style: Style,
    border_style: Style,
) {
    let dim = theme::muted();
    let width = block_area.width as usize;
    let label_w = label.width();
    let fill = width.saturating_sub(3 + label_w);
    let line = Line::from(vec![
        Span::styled("├", border_style),
        Span::styled("─", dim),
        Span::styled(label.to_string(), label_style),
        Span::styled("─".repeat(fill), dim),
        Span::styled("┤", border_style),
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
