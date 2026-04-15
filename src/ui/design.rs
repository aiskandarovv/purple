//! Design system tokens and reusable component builders.
//!
//! This module centralizes spacing, overlay sizing, toast, timeout, icon and
//! list rendering constants that are shared across UI modules. It also exposes
//! block component builders, layout helpers, a `Footer` builder and a small
//! set of render helpers so individual screens can stay short and consistent.
//!
//! The goal is to keep design intent in one place and have screens reference
//! these helpers instead of duplicating border, title or footer wiring.
//!
//! Task 1 introduces this module as a foundation. Individual screens are
//! migrated to use these helpers in follow-up tasks, so items here may appear
//! unused by the binary until then.

#![allow(dead_code)]

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use super::theme;
use crate::app::App;

// ---------------------------------------------------------------------------
// Spacing tokens
// ---------------------------------------------------------------------------

/// Two-space gap used between footer action entries.
pub const FOOTER_GAP: &str = "  ";
/// Gap between columns in list rows.
pub const COL_GAP: u16 = 2;
/// Horizontal indent used inside form blocks.
pub const FORM_INDENT: u16 = 1;

// ---------------------------------------------------------------------------
// Overlay sizing tokens
// ---------------------------------------------------------------------------

/// Default overlay width percentage.
pub const OVERLAY_W: u16 = 70;
/// Default overlay height percentage.
pub const OVERLAY_H: u16 = 80;
/// Default overlay margin reservation.
pub const OVERLAY_MARGIN: u16 = 4;
/// Minimum width used for simple list pickers.
pub const PICKER_MIN_W: u16 = 50;
/// Maximum width used for simple list pickers.
pub const PICKER_MAX_W: u16 = 64;

// ---------------------------------------------------------------------------
// Toast tokens
// ---------------------------------------------------------------------------

/// Toast horizontal inset from the right edge.
pub const TOAST_INSET_X: u16 = 2;
/// Toast vertical inset from the bottom edge.
pub const TOAST_INSET_Y: u16 = 2;
/// Toast success glyph (U+2713, standard Unicode check mark).
pub const TOAST_ICON_OK: &str = "\u{2713}";
/// Toast alert glyph (U+26A0, standard Unicode warning sign).
pub const TOAST_ICON_ALERT: &str = "\u{26A0}";

// ---------------------------------------------------------------------------
// Timeout tokens (1 tick = 250ms)
// ---------------------------------------------------------------------------

/// Ticks before a confirmation toast clears (4s).
pub const TIMEOUT_CONFIRM: u32 = 16;
/// Ticks before an info toast clears (4s).
pub const TIMEOUT_INFO: u32 = 16;
/// Ticks before an alert toast clears (5s).
pub const TIMEOUT_ALERT: u32 = 20;
/// Maximum number of queued toast messages.
pub const TOAST_QUEUE_MAX: usize = 5;

// ---------------------------------------------------------------------------
// Status indicator tokens
// ---------------------------------------------------------------------------

/// Online status glyph (U+25CF, filled circle).
pub const ICON_ONLINE: &str = "\u{25CF}";
/// Success glyph (U+2713, check mark).
pub const ICON_SUCCESS: &str = "\u{2713}";
/// Warning glyph (U+26A0, warning sign).
pub const ICON_WARNING: &str = "\u{26A0}";

// ---------------------------------------------------------------------------
// List rendering tokens
// ---------------------------------------------------------------------------

/// Default list-row highlight prefix (two spaces).
pub const LIST_HIGHLIGHT: &str = "  ";
/// Host list highlight prefix (U+258C, left half block).
pub const HOST_HIGHLIGHT: &str = "\u{258C}";

// ---------------------------------------------------------------------------
// Detail panel tokens
// ---------------------------------------------------------------------------

/// Detail panel section label column width.
pub const SECTION_LABEL_W: u16 = 14;

// ---------------------------------------------------------------------------
// Dim background tokens
// ---------------------------------------------------------------------------

/// RGB triple used for dim-background text.
pub const DIM_FG_RGB: (u8, u8, u8) = (70, 70, 70);

// ---------------------------------------------------------------------------
// Column-width helper
// ---------------------------------------------------------------------------

/// Column-width padding formula used by list columns.
pub fn padded(w: u16) -> u16 {
    if w == 0 { 0 } else { w + w / 10 + 1 }
}

// ---------------------------------------------------------------------------
// Block component builders
// ---------------------------------------------------------------------------

/// Standard overlay block: rounded border, brand title, accent border.
pub fn overlay_block(title: &str) -> Block<'static> {
    overlay_block_line(Line::from(Span::styled(
        format!(" {title} "),
        theme::brand(),
    )))
}

/// Overlay block variant accepting a pre-built compound title `Line`.
/// Use when the caller needs multi-span titles that `overlay_block(&str)`
/// cannot express. Border style, border type and borders match `overlay_block`.
pub fn overlay_block_line(title: Line<'static>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::accent())
        .title(title)
}

/// Plain overlay block: rounded border, accent border, NO title. Use for
/// unique dialogs (e.g. welcome screen) where the block carries no title
/// and the content itself supplies visual hierarchy.
pub fn plain_overlay_block() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::accent())
}

/// Danger overlay block: rounded border, danger title, danger border.
/// Use for destructive confirmations (delete, purge).
pub fn danger_block(title: &str) -> Block<'static> {
    danger_block_line(Line::from(Span::styled(
        format!(" {title} "),
        theme::danger(),
    )))
}

/// Danger block variant accepting a pre-built compound title `Line`.
pub fn danger_block_line(title: Line<'static>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_danger())
        .title(title)
}

/// Main screen block: rounded border, brand title, dim border.
pub fn main_block(title: &str) -> Block<'static> {
    main_block_line(Line::from(Span::styled(
        format!(" {title} "),
        theme::brand(),
    )))
}

/// Main block variant accepting a pre-built compound title `Line`.
/// Use when the caller needs multi-span titles that `main_block(&str)`
/// cannot express (e.g. the host list's `[ALL] hosts (42) + filter badges`).
pub fn main_block_line(title: Line<'static>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(title)
}

/// Search-active block: rounded border, brand title, search border.
pub fn search_block(title: &str) -> Block<'static> {
    search_block_line(Line::from(Span::styled(
        format!(" {title} "),
        theme::brand(),
    )))
}

/// Search block variant accepting a pre-built compound title `Line`.
/// Mirrors `main_block_line` but with the search border style.
pub fn search_block_line(title: Line<'static>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_search())
        .title(title)
}

// ---------------------------------------------------------------------------
// Layout helpers
// ---------------------------------------------------------------------------

/// Overlay area: percentage width with a fixed height clamped to terminal.
pub fn overlay_area(frame: &Frame, w_pct: u16, h_pct: u16, height: u16) -> Rect {
    let area = frame.area();
    // Start from a percentage-based rectangle, then clamp the vertical extent
    // to the caller-requested height so narrow terminals still show a usable
    // overlay without stretching vertically.
    let pct_area = super::centered_rect(w_pct, h_pct, area);
    super::centered_rect_fixed(pct_area.width, height.min(pct_area.height), area)
}

/// Content + footer with a 1-row spacer in between. Returns `(content, footer)`.
pub fn content_and_footer(inner: Rect) -> (Rect, Rect) {
    let (content, _spacer, footer) = content_spacer_footer(inner);
    (content, footer)
}

/// Content + spacer + footer. Returns all three rects.
pub fn content_spacer_footer(inner: Rect) -> (Rect, Rect, Rect) {
    let [content, spacer, footer] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas::<3>(inner);
    (content, spacer, footer)
}

/// Form footer positioned directly below the block border.
pub fn form_footer(block_area: Rect, block_height: u16) -> Rect {
    Rect::new(
        block_area.x,
        block_area.y + block_height,
        block_area.width,
        1,
    )
}

/// Form field content area at the given index.
pub fn form_field_area(inner: Rect, index: usize) -> Rect {
    Rect::new(
        inner.x + FORM_INDENT,
        inner.y + (index as u16) * 2,
        inner.width.saturating_sub(FORM_INDENT * 2),
        1,
    )
}

/// Form divider Y position for the given index.
pub fn form_divider_y(inner: Rect, index: usize) -> u16 {
    inner.y + (index as u16) * 2
}

/// Picker overlay width clamped to `[PICKER_MIN_W, PICKER_MAX_W]`.
///
/// Canonical formula used by all picker overlays (ProxyJump, Vault role,
/// Password source). `super::picker_overlay_width` delegates here.
pub fn picker_width(frame: &Frame) -> u16 {
    frame.area().width.clamp(PICKER_MIN_W, PICKER_MAX_W)
}

// ---------------------------------------------------------------------------
// Footer builder
// ---------------------------------------------------------------------------

/// Builder for action footers. Inserts `FOOTER_GAP` between entries only.
pub struct Footer {
    spans: Vec<Span<'static>>,
}

impl Footer {
    /// Create an empty footer.
    pub fn new() -> Self {
        Self { spans: Vec::new() }
    }

    /// Add a primary action (semantic marker for the default action).
    #[allow(deprecated)]
    pub fn primary(mut self, key: &str, label: &str) -> Self {
        if !self.spans.is_empty() {
            self.spans.push(Span::raw(FOOTER_GAP));
        }
        let [k, l] = super::footer_primary(key, label);
        self.spans.push(k);
        self.spans.push(l);
        self
    }

    /// Add a secondary action.
    pub fn action(mut self, key: &str, label: &str) -> Self {
        if !self.spans.is_empty() {
            self.spans.push(Span::raw(FOOTER_GAP));
        }
        let [k, l] = super::footer_action(key, label);
        self.spans.push(k);
        self.spans.push(l);
        self
    }

    /// Render in an overlay footer (status right-aligned if present).
    pub fn render_with_status(self, frame: &mut Frame, area: Rect, app: &App) {
        super::render_footer_with_status(frame, area, self.spans, app);
    }

    /// Render in a main screen footer (with the "? more" hint on the right).
    pub fn render_with_help(self, frame: &mut Frame, area: Rect, app: &App) {
        super::render_footer_with_help(frame, area, self.spans, app);
    }

    /// Convert the accumulated spans into a single `Line`.
    #[allow(clippy::wrong_self_convention)]
    pub fn to_line(self) -> Line<'static> {
        Line::from(self.spans)
    }

    /// Raw spans for screens with custom footer rendering.
    pub fn into_spans(self) -> Vec<Span<'static>> {
        self.spans
    }
}

impl Default for Footer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Render helpers
// ---------------------------------------------------------------------------

/// Render a 2-space-indented message with the muted style.
fn render_muted_message(frame: &mut Frame, area: Rect, message: &str) {
    let line = Line::from(vec![
        Span::raw("  "),
        Span::styled(message.to_string(), theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/// Render an empty-state message with 2-space indent and muted style.
pub fn render_empty(frame: &mut Frame, area: Rect, message: &str) {
    render_muted_message(frame, area, message);
}

/// Render a loading message with 2-space indent and muted style.
pub fn render_loading(frame: &mut Frame, area: Rect, message: &str) {
    render_muted_message(frame, area, message);
}

/// Render an error message with 2-space indent and error style.
pub fn render_error(frame: &mut Frame, area: Rect, message: &str) {
    let line = Line::from(vec![
        Span::raw("  "),
        Span::styled(message.to_string(), theme::error()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/// Render a column header row. Bold style is applied to the paragraph itself.
pub fn render_column_header(frame: &mut Frame, area: Rect, spans: Vec<Span<'_>>) {
    let owned: Vec<Span<'static>> = spans
        .into_iter()
        .map(|s| Span::styled(s.content.into_owned(), s.style))
        .collect();
    let paragraph = Paragraph::new(Line::from(owned)).style(theme::bold());
    frame.render_widget(paragraph, area);
}

/// Inline section divider below section headers.
/// Renders as indented dashes in muted style.
pub fn section_divider() -> Line<'static> {
    Line::from(Span::styled("  ────────────────────────", theme::muted()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::widgets::Widget;

    fn make_app() -> (App, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::ssh_config::model::SshConfigFile {
            elements: crate::ssh_config::model::SshConfigFile::parse_content(""),
            path: dir.path().join("test_design"),
            crlf: false,
            bom: false,
        };
        (App::new(config), dir)
    }

    fn buffer_contains(buf: &Buffer, needle: &str) -> bool {
        for y in 0..buf.area.height {
            let mut row = String::new();
            for x in 0..buf.area.width {
                row.push_str(buf[(x, y)].symbol());
            }
            if row.contains(needle) {
                return true;
            }
        }
        false
    }

    fn render_block_title(block: Block<'static>, title: &str) -> bool {
        let area = Rect::new(0, 0, 30, 5);
        let mut buf = Buffer::empty(area);
        block.render(area, &mut buf);
        buffer_contains(&buf, title)
    }

    #[test]
    fn overlay_block_title_is_padded() {
        assert!(render_block_title(overlay_block("Hello"), " Hello "));
    }

    #[test]
    fn danger_block_title_is_padded() {
        assert!(render_block_title(danger_block("Delete"), " Delete "));
    }

    #[test]
    fn main_block_title_is_padded() {
        assert!(render_block_title(main_block("Hosts"), " Hosts "));
    }

    #[test]
    fn search_block_title_is_padded() {
        assert!(render_block_title(search_block("Search"), " Search "));
    }

    #[test]
    fn overlay_area_stays_within_frame() {
        let backend = TestBackend::new(100, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let rect = overlay_area(frame, 70, 80, 20);
                let area = frame.area();
                assert!(rect.x >= area.x);
                assert!(rect.y >= area.y);
                assert!(rect.x + rect.width <= area.x + area.width);
                assert!(rect.y + rect.height <= area.y + area.height);
                assert!(rect.height <= 20);
            })
            .unwrap();
    }

    #[test]
    fn content_and_footer_heights_sum_with_spacer() {
        let inner = Rect::new(0, 0, 40, 10);
        let (content, footer) = content_and_footer(inner);
        assert_eq!(footer.height, 1);
        assert_eq!(content.height + 1 + footer.height, 10);
    }

    #[test]
    fn content_spacer_footer_returns_three_rects_summing_to_total() {
        let inner = Rect::new(0, 0, 40, 10);
        let (content, spacer, footer) = content_spacer_footer(inner);
        assert_eq!(spacer.height, 1);
        assert_eq!(footer.height, 1);
        assert_eq!(content.height + spacer.height + footer.height, 10);
    }

    #[test]
    fn form_footer_sits_directly_below_block() {
        let block_area = Rect::new(5, 2, 30, 8);
        let rect = form_footer(block_area, 8);
        assert_eq!(rect.x, 5);
        assert_eq!(rect.y, 10);
        assert_eq!(rect.width, 30);
        assert_eq!(rect.height, 1);
    }

    #[test]
    fn form_field_area_steps_by_two() {
        let inner = Rect::new(2, 3, 20, 10);
        assert_eq!(form_field_area(inner, 0).y, 3);
        assert_eq!(form_field_area(inner, 1).y, 5);
        assert_eq!(form_field_area(inner, 2).y, 7);
    }

    #[test]
    fn form_divider_y_steps_by_two() {
        let inner = Rect::new(2, 3, 20, 10);
        assert_eq!(form_divider_y(inner, 0), 3);
        assert_eq!(form_divider_y(inner, 1), 5);
        assert_eq!(form_divider_y(inner, 2), 7);
    }

    #[test]
    fn padded_matches_expected_values() {
        assert_eq!(padded(0), 0);
        assert_eq!(padded(10), 12);
        assert_eq!(padded(20), 23);
    }

    #[test]
    fn footer_builder_inserts_gaps_between_entries_only() {
        let spans = Footer::new()
            .primary("Enter", "save")
            .action("Esc", "cancel")
            .action("Tab", "next")
            .into_spans();
        // primary (2) + gap (1) + action (2) + gap (1) + action (2) = 8
        assert_eq!(spans.len(), 8);
        assert_eq!(spans[2].content, FOOTER_GAP);
        assert_eq!(spans[5].content, FOOTER_GAP);
    }

    #[test]
    fn empty_footer_has_no_spans() {
        assert!(Footer::new().into_spans().is_empty());
    }

    #[test]
    fn footer_to_line_preserves_span_count() {
        let footer = Footer::new()
            .primary("Enter", "save")
            .action("Esc", "cancel");
        let spans_len = {
            let clone = Footer::new()
                .primary("Enter", "save")
                .action("Esc", "cancel");
            clone.into_spans().len()
        };
        let line = footer.to_line();
        assert_eq!(line.spans.len(), spans_len);
    }

    #[test]
    fn render_column_header_does_not_panic() {
        let backend = TestBackend::new(40, 2);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 40, 1);
                let spans = vec![Span::raw("alias"), Span::raw("host")];
                render_column_header(frame, area, spans);
            })
            .unwrap();
    }

    #[test]
    fn picker_width_is_clamped() {
        let backend = TestBackend::new(100, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let w = picker_width(frame);
                assert!(w >= PICKER_MIN_W);
                assert!(w <= PICKER_MAX_W);
            })
            .unwrap();
    }

    #[test]
    fn picker_width_clamps_narrow_terminal_to_min() {
        let backend = TestBackend::new(30, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                assert_eq!(picker_width(frame), PICKER_MIN_W);
            })
            .unwrap();
    }

    #[test]
    fn picker_width_clamps_wide_terminal_to_max() {
        let backend = TestBackend::new(200, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                assert_eq!(picker_width(frame), PICKER_MAX_W);
            })
            .unwrap();
    }

    #[test]
    fn picker_width_passes_midrange_through() {
        let backend = TestBackend::new(58, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                assert_eq!(picker_width(frame), 58);
            })
            .unwrap();
    }

    #[test]
    fn plain_overlay_block_has_no_title() {
        // Render the block into a small buffer and verify the top border row
        // contains only rounded glyphs and horizontal lines (no injected title
        // characters from a helper).
        let area = Rect::new(0, 0, 20, 3);
        let mut buf = Buffer::empty(area);
        plain_overlay_block().render(area, &mut buf);
        let mut top = String::new();
        for x in 0..area.width {
            top.push_str(buf[(x, 0)].symbol());
        }
        assert!(top.starts_with('\u{256D}'));
        assert!(top.ends_with('\u{256E}'));
        // All inner chars should be box-drawing horizontals.
        for ch in top.chars().skip(1).take((area.width as usize) - 2) {
            assert_eq!(ch, '\u{2500}');
        }
    }

    #[test]
    fn section_divider_contains_dashes() {
        let line = section_divider();
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("────"),
            "section divider should contain dash characters"
        );
    }

    #[test]
    fn render_empty_loading_error_do_not_panic() {
        let backend = TestBackend::new(40, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 40, 1);
                render_empty(frame, area, "no hosts");
                render_loading(frame, area, "loading...");
                render_error(frame, area, "something broke");
            })
            .unwrap();
    }

    #[test]
    fn footer_render_with_status_does_not_panic() {
        let (app, _dir) = make_app();
        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 60, 1);
                Footer::new()
                    .primary("Enter", "save")
                    .action("Esc", "cancel")
                    .render_with_status(frame, area, &app);
            })
            .unwrap();
    }
}
