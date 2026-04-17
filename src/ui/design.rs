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
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use super::theme;
use crate::app::App;

// ---------------------------------------------------------------------------
// Spacing tokens
// ---------------------------------------------------------------------------

/// Two-space gap used between footer action entries.
pub const FOOTER_GAP: &str = "  ";
/// Gap between columns in list rows.
pub const COL_GAP: u16 = 2;

/// Canonical lowercase "purple" wordmark (6 lines, figlet Standard font).
/// Single source of truth for every overlay that shows the brand.
pub const LOGO: [&str; 6] = [
    "                        _      ",
    " _ __  _   _ _ __ _ __ | | ___ ",
    "| '_ \\| | | | '__| '_ \\| |/ _ \\",
    "| |_) | |_| | |  | |_) | |  __/",
    "| .__/ \\__,_|_|  | .__/|_|\\___|",
    "|_|              |_|           ",
];

// ---------------------------------------------------------------------------
// Overlay sizing tokens
// ---------------------------------------------------------------------------

/// Default overlay width percentage.
pub const OVERLAY_W: u16 = 70;
/// Default overlay height percentage.
pub const OVERLAY_H: u16 = 80;
/// Minimum width for picker overlays. All pickers (Password Source,
/// Select Key, Vault SSH Role, ProxyJump, tag picker, theme picker, etc.)
/// share this single sizing range so they look identical regardless of
/// which form field opened them.
pub const PICKER_MIN_W: u16 = 60;
/// Maximum width for picker overlays.
pub const PICKER_MAX_W: u16 = 72;
/// Maximum height (incl. borders) for picker overlays. Pickers grow with
/// item count up to this cap, then scroll.
pub const PICKER_MAX_H: u16 = 18;

// ---------------------------------------------------------------------------
// Toast tokens
// ---------------------------------------------------------------------------

/// Toast horizontal inset from the right edge.
pub const TOAST_INSET_X: u16 = 2;
/// Toast vertical inset from the bottom edge.
pub const TOAST_INSET_Y: u16 = 2;

// ---------------------------------------------------------------------------
// Timeout tokens (millisecond-based, tick-rate-independent)
// ---------------------------------------------------------------------------

/// Minimum milliseconds before a Success or Info message clears (2.5s).
/// Effective timeout is `max(TIMEOUT_MIN_MS, words * MS_PER_WORD)`.
pub const TIMEOUT_MIN_MS: u64 = 2500;
/// Minimum milliseconds before a Warning message clears (4s).
pub const TIMEOUT_MIN_WARNING_MS: u64 = 4000;
/// Per-word reading-time budget in milliseconds (750ms/word, matching
/// peripheral reading speed for short status strings competing with the
/// primary task).
pub const MS_PER_WORD: u64 = 750;
/// Cap on word count for length-proportional timeout. 30 words at
/// 750ms/word = 22.5s maximum for any non-sticky toast.
pub const WORD_CAP: usize = 30;
/// Maximum number of queued toast messages. Three matches Linear/Stripe
/// toast stack patterns; more than 3 stacked toasts is itself a UX signal
/// of a system problem and dropping older ones is preferable to clutter.
pub const TOAST_QUEUE_MAX: usize = 3;

// ---------------------------------------------------------------------------
// Status indicator tokens
// ---------------------------------------------------------------------------

/// Online status glyph (U+25CF, filled circle).
pub const ICON_ONLINE: &str = "\u{25CF}";
/// Success glyph (U+2713, check mark). Also used as the toast success glyph.
pub const ICON_SUCCESS: &str = "\u{2713}";
/// Warning glyph (U+26A0, warning sign). Also used as the toast warning glyph.
pub const ICON_WARNING: &str = "\u{26A0}";
/// Error glyph (U+2716, heavy multiplication X). Distinct from the
/// warning sign so the user can tell at a glance whether something is
/// recoverable (warning) or has gone wrong (error).
pub const ICON_ERROR: &str = "\u{2716}";

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

/// Search-active block accepting a pre-built compound title `Line`.
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

/// Form footer positioned directly below the block border.
///
/// All overlays use this — there is no longer an "inside the block + spacer"
/// alternative. Form screens, list/picker overlays and detail overlays
/// alike render their action footer at this fixed external position so the
/// keycaps strip lines up consistently across every screen.
///
/// **Note:** Prefer `render_overlay_footer` over this helper. `form_footer`
/// only computes the Rect; `render_overlay_footer` also renders a `Clear`
/// widget over the footer row so it does not show through to the screen
/// behind the overlay (e.g. the host list when a picker is open).
pub fn form_footer(block_area: Rect, block_height: u16) -> Rect {
    Rect::new(
        block_area.x,
        block_area.y + block_height,
        block_area.width,
        1,
    )
}

/// Compute the external footer Rect for an overlay block, render `Clear`
/// over it so the row underneath the overlay does not bleed through, and
/// return the footer Rect for the caller to render the footer spans into.
pub fn render_overlay_footer(frame: &mut Frame, block_area: Rect) -> Rect {
    let footer_area = form_footer(block_area, block_area.height);
    frame.render_widget(Clear, footer_area);
    footer_area
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

/// 2-space-indented muted line. Single source of truth for the
/// indent + muted style pattern shared by `render_empty`, `render_loading`
/// and `empty_line`.
fn muted_line(message: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(message.to_string(), theme::muted()),
    ])
}

/// Render a 2-space-indented message with the muted style.
fn render_muted_message(frame: &mut Frame, area: Rect, message: &str) {
    frame.render_widget(Paragraph::new(muted_line(message)), area);
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

/// Inline section divider below section headers.
/// Renders as indented dashes in muted style.
pub fn section_divider() -> Line<'static> {
    Line::from(Span::styled("  ────────────────────────", theme::muted()))
}

// ---------------------------------------------------------------------------
// Content-level helpers
// ---------------------------------------------------------------------------

/// Column-width padding formula (usize variant for list screens).
pub fn padded_usize(w: usize) -> usize {
    if w == 0 { 0 } else { w + w / 10 + 1 }
}

/// 3-space prefix for column headers (aligns with highlight_symbol + leading space).
pub const COLUMN_HEADER_PREFIX: &str = "   ";

/// Inter-column gap as string.
pub const COL_GAP_STR: &str = "  ";

/// Key-value line: muted label (left-padded to width) + bold value.
pub fn kv_line(label: &str, value: &str, label_width: usize) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<width$}", label, width = label_width),
            theme::muted(),
        ),
        Span::styled(value.to_string(), theme::bold()),
    ])
}

/// Key-value label width for overlay detail screens (host_detail, key_detail).
pub const KV_LABEL_WIDE: usize = 22;

/// Content section header + divider pair.
pub fn content_section(label: &str) -> [Line<'static>; 2] {
    [
        Line::from(vec![
            Span::raw("  "),
            Span::styled(label.to_string(), theme::section_header()),
        ]),
        section_divider(),
    ]
}

/// Empty state with action hint: `"  message  \[key\]  action"`
pub fn render_empty_with_hint(
    frame: &mut Frame,
    area: Rect,
    message: &str,
    key: &str,
    action: &str,
) {
    let line = Line::from(vec![
        Span::raw("  "),
        Span::styled(message.to_string(), theme::muted()),
        Span::raw("  "),
        Span::styled(format!(" {} ", key), theme::footer_key()),
        Span::styled(format!(" {}", action), theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/// Right-arrow glyph for picker fields.
pub const PICKER_ARROW: &str = "\u{25B8}";

/// Space-bar glyph for toggle fields.
pub const TOGGLE_HINT: &str = "\u{2423}";

/// Empty-state line for embedding in Paragraphs that render inside a block.
/// Same visual output as `render_empty()` but returns a composable `Line`.
pub fn empty_line(message: &str) -> Line<'static> {
    muted_line(message)
}

// ---------------------------------------------------------------------------
// Keyboard interaction primitives
// ---------------------------------------------------------------------------
//
// These helpers are the single source of truth for keyboard interaction
// patterns in purple. The CI script `scripts/check-keybindings.sh` enforces
// that handler and screen code uses these helpers instead of building footers
// or routing keys ad hoc.

/// Field kind for dynamic form footer hints.
///
/// Drives the `Space` action label in [`form_save_footer`]:
/// - `Text`: Space inserts a literal space character. No hint shown.
/// - `Toggle`: Space flips a boolean. Footer shows "Space toggle".
/// - `Picker`: Space opens a selection picker. Footer shows "Space pick".
///
/// **Invariant**: Enter ALWAYS submits the form regardless of `FieldKind`.
/// Pickers and toggles are reached via Space only, never via Enter.
/// `scripts/check-keybindings.sh` enforces this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Text input field. Space inserts a literal character.
    Text,
    /// Boolean toggle (e.g. VerifyTls, AutoSync). Space flips the value.
    Toggle,
    /// Picker field (e.g. IdentityFile, ProxyJump). Space opens the picker.
    Picker,
}

/// Form mode for dynamic footer rendering.
///
/// Forms with progressive disclosure (host form, provider form) start
/// `Collapsed` showing only required fields. The footer hints `\u{2193} more
/// options` so the user can expand. After expansion the footer flips to
/// `Expanded(kind)` and shows the appropriate per-field hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormFooterMode {
    /// Required fields only. Down arrow expands to optional fields.
    Collapsed,
    /// All fields visible. Field kind determines the Space hint.
    Expanded(FieldKind),
}

/// Standard form save footer with dynamic hints based on focused field.
///
/// Renders one of:
/// - Collapsed:                `Enter save | \u{2193} more options | Esc cancel`
/// - Expanded + Text field:    `Enter save | Tab next | Esc cancel`
/// - Expanded + Toggle field:  `Enter save | Space toggle | Tab next | Esc cancel`
/// - Expanded + Picker field:  `Enter save | Space pick | Tab next | Esc cancel`
///
/// **Why this helper exists**: it codifies the rule that Enter is always the
/// save action, and that Space is the universal field-action key. Screens
/// must call this instead of building form footers ad hoc.
pub fn form_save_footer(mode: FormFooterMode) -> Footer {
    let mut footer = Footer::new().primary("Enter", " save ");
    match mode {
        FormFooterMode::Collapsed => {
            footer = footer.action("\u{2193}", " more options ");
        }
        FormFooterMode::Expanded(FieldKind::Text) => {
            footer = footer.action("Tab", " next ");
        }
        FormFooterMode::Expanded(FieldKind::Toggle) => {
            footer = footer.action("Space", " toggle ").action("Tab", " next ");
        }
        FormFooterMode::Expanded(FieldKind::Picker) => {
            footer = footer.action("Space", " pick ").action("Tab", " next ");
        }
    }
    footer.action("Esc", " cancel")
}

/// Footer for a destructive confirmation. Action-specific verbs both sides.
///
/// Stakes test: if cancelling by mistake loses irrecoverable work, use
/// action verbs (e.g. `delete/keep`, `sign/skip`, `purge/keep`). The
/// asymmetry helps users read the dialog as a choice between two outcomes,
/// not "did I press the right key?".
///
/// Both `n` and `Esc` cancel (the contract enforced by
/// `handler::route_confirm_key`); the footer advertises them as `n/Esc` so
/// the visible UI matches the actual key set.
///
/// Examples:
/// - `confirm_footer_destructive("delete", "keep")` for delete confirms
/// - `confirm_footer_destructive("sign", "skip")` for vault sign
/// - `confirm_footer_destructive("purge", "keep")` for purge stale
pub fn confirm_footer_destructive(yes_verb: &str, no_verb: &str) -> Footer {
    Footer::new()
        .primary("y", &format!(" {} ", yes_verb))
        .action("n/Esc", &format!(" {}", no_verb))
}

/// Footer for the standard discard-changes confirmation in any form.
///
/// Discarding form changes is a benign confirmation: users can re-enter the
/// data. We still use action verbs (`discard`/`keep`) instead of `yes/no`
/// because the noun-verb pairing is more informative than a bare affirmative.
pub fn discard_footer() -> Footer {
    confirm_footer_destructive("discard", "keep")
}

/// Render the standard "Discard changes?" footer with prompt prefix.
///
/// Single source of truth for the discard prompt across every editable
/// surface (host form, tunnel form, snippet form, provider form, snippet
/// param form, bulk tag editor). Renders below the block via
/// `render_overlay_footer`. Callers must compute `footer_area` first via
/// [`render_overlay_footer`] and pass it in.
pub fn render_discard_prompt(frame: &mut Frame, footer_area: Rect, app: &App) {
    let mut spans = vec![Span::styled(" Discard changes? ", theme::error())];
    spans.extend(discard_footer().into_spans());
    super::render_footer_with_status(frame, footer_area, spans, app);
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
    fn form_footer_sits_directly_below_block() {
        let block_area = Rect::new(5, 2, 30, 8);
        let rect = form_footer(block_area, 8);
        assert_eq!(rect.x, 5);
        assert_eq!(rect.y, 10);
        assert_eq!(rect.width, 30);
        assert_eq!(rect.height, 1);
    }

    #[test]
    fn form_divider_y_steps_by_two() {
        let inner = Rect::new(2, 3, 20, 10);
        assert_eq!(form_divider_y(inner, 0), 3);
        assert_eq!(form_divider_y(inner, 1), 5);
        assert_eq!(form_divider_y(inner, 2), 7);
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
        // PICKER_MIN_W (60) < 66 < PICKER_MAX_W (72), so passes through unclamped.
        let backend = TestBackend::new(66, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                assert_eq!(picker_width(frame), 66);
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
    fn padded_usize_matches_expected_values() {
        assert_eq!(padded_usize(0), 0);
        assert_eq!(padded_usize(10), 12);
        assert_eq!(padded_usize(20), 23);
    }

    #[test]
    fn kv_line_format_has_two_spans() {
        let line = kv_line("Label", "Value", KV_LABEL_WIDE);
        assert_eq!(line.spans.len(), 2);
        let label_text = &line.spans[0].content;
        assert!(
            label_text.starts_with("  "),
            "label should be 2-space indented"
        );
        assert!(label_text.contains("Label"));
        assert_eq!(line.spans[1].content.as_ref(), "Value");
    }

    #[test]
    fn kv_line_label_is_padded_to_width() {
        let line = kv_line("X", "Y", 22);
        let label = &line.spans[0].content;
        // 2-space indent + 22-char padded label = 24 total
        assert_eq!(label.len(), 24);
    }

    #[test]
    fn content_section_returns_header_and_divider() {
        let [header, divider] = content_section("Directives");
        let h_text: String = header.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(h_text.contains("Directives"));
        let d_text: String = divider.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(d_text.contains("────"));
    }

    #[test]
    fn render_empty_with_hint_does_not_panic() {
        let backend = TestBackend::new(60, 3);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, 60, 1);
                render_empty_with_hint(frame, area, "No tags yet.", "+", "add");
            })
            .unwrap();
    }

    #[test]
    fn column_header_prefix_is_three_spaces() {
        assert_eq!(COLUMN_HEADER_PREFIX, "   ");
        assert_eq!(COLUMN_HEADER_PREFIX.len(), 3);
    }

    #[test]
    fn col_gap_str_is_two_spaces() {
        assert_eq!(COL_GAP_STR, "  ");
        assert_eq!(COL_GAP_STR.len(), 2);
    }

    #[test]
    fn picker_arrow_renders_as_single_glyph() {
        // The grep check in scripts/check-design-system.sh enforces that the
        // literal "\u{25B8}" only appears in design.rs. The test here
        // guards a different invariant: PICKER_ARROW must be a single
        // non-whitespace grapheme so it lines up in form fields.
        assert_eq!(PICKER_ARROW.chars().count(), 1);
        assert!(!PICKER_ARROW.starts_with(char::is_whitespace));
    }

    #[test]
    fn toggle_hint_renders_as_single_glyph() {
        assert_eq!(TOGGLE_HINT.chars().count(), 1);
        assert!(!TOGGLE_HINT.starts_with(char::is_whitespace));
    }

    #[test]
    fn empty_line_has_indent_and_muted_style() {
        let line = empty_line("No results.");
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content.as_ref(), "  ");
        assert_eq!(line.spans[1].content.as_ref(), "No results.");
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

    fn footer_text(footer: Footer) -> String {
        footer
            .into_spans()
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn form_save_footer_collapsed_shows_more_options() {
        let text = footer_text(form_save_footer(FormFooterMode::Collapsed));
        assert!(text.contains("Enter"));
        assert!(text.contains("save"));
        assert!(text.contains("more options"));
        assert!(text.contains("Esc"));
        assert!(text.contains("cancel"));
        // Collapsed mode never advertises Space.
        assert!(!text.contains("Space"));
    }

    #[test]
    fn form_save_footer_expanded_text_omits_space_hint() {
        let text = footer_text(form_save_footer(FormFooterMode::Expanded(FieldKind::Text)));
        assert!(text.contains("Enter"));
        assert!(text.contains("save"));
        assert!(text.contains("Tab"));
        assert!(text.contains("Esc"));
        // Text fields: Space is a literal character, not a hint.
        assert!(!text.contains("Space"));
    }

    #[test]
    fn form_save_footer_expanded_toggle_shows_space_toggle() {
        let text = footer_text(form_save_footer(FormFooterMode::Expanded(
            FieldKind::Toggle,
        )));
        assert!(text.contains("Space"));
        assert!(text.contains("toggle"));
        // Should not advertise picker on a toggle field.
        assert!(!text.contains("pick"));
    }

    #[test]
    fn form_save_footer_expanded_picker_shows_space_pick() {
        let text = footer_text(form_save_footer(FormFooterMode::Expanded(
            FieldKind::Picker,
        )));
        assert!(text.contains("Space"));
        assert!(text.contains("pick"));
        // Should not advertise toggle on a picker field.
        assert!(!text.contains("toggle"));
    }

    #[test]
    fn confirm_footer_destructive_uses_action_verbs() {
        let text = footer_text(confirm_footer_destructive("delete", "keep"));
        assert!(text.contains("y"));
        assert!(text.contains("delete"));
        assert!(text.contains("n/Esc"));
        assert!(text.contains("keep"));
        // Destructive footer must not contain generic yes/no labels.
        assert!(!text.contains("yes"));
        assert!(!text.contains(" no"));
    }

    #[test]
    fn confirm_footers_advertise_n_alongside_esc() {
        // route_confirm_key accepts y/Y, n/N, Esc. The footer must advertise
        // both n and Esc to keep the visible UI in sync with the key contract.
        for footer_text_str in [
            footer_text(confirm_footer_destructive("delete", "keep")),
            footer_text(discard_footer()),
        ] {
            assert!(
                footer_text_str.contains("n/Esc"),
                "footer must show both n and Esc as cancel keys: {}",
                footer_text_str
            );
        }
    }

    #[test]
    fn discard_footer_uses_discard_keep_verbs() {
        let text = footer_text(discard_footer());
        assert!(text.contains("discard"));
        assert!(text.contains("keep"));
    }
}
