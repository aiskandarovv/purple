use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{self, App, HostListItem, PingStatus, ViewMode};
use crate::ssh_config::model::ConfigElement;

/// Minimum terminal width to show the detail panel in detailed view mode.
const DETAIL_MIN_WIDTH: u16 = 95;

/// Build the update badge label, truncating the headline with ellipsis if needed.
/// `max_width` is the border area width (including border chars).
fn build_update_label(ver: &str, headline: Option<&str>, hint: &str, max_width: u16) -> String {
    // Budget: area width minus 2 border chars and 1 char padding on each side
    let budget = (max_width as usize).saturating_sub(4);
    match headline {
        Some(hl) => {
            let full = format!(" v{}: {} (run {}) ", ver, hl, hint);
            if full.width() <= budget {
                return full;
            }
            // Truncate headline to fit
            let prefix = format!(" v{}: ", ver);
            let suffix = format!(" (run {}) ", hint);
            let hl_budget = budget
                .saturating_sub(prefix.width())
                .saturating_sub(suffix.width());
            if hl_budget >= 4 {
                let hl_trunc = super::truncate(hl, hl_budget);
                format!("{}{}{}", prefix, hl_trunc, suffix)
            } else {
                // Not enough room for headline: fall back to version-only
                format!(" v{} available, run {} ", ver, hint)
            }
        }
        None => format!(" v{} available, run {} ", ver, hint),
    }
}

const HOST_MIN: usize = 12;
/// Width of the row marker (indent + selection checkmark space).
const MARKER_WIDTH: usize = 2;

/// Column layout computed from the visible host list.
struct Columns {
    alias: usize,
    host: usize,
    tags: usize,
    tunnel: usize,
    auth: usize,
    show_ping: bool,
    history: usize,
    gap: usize,
    /// Flexible gap between left cluster (NAME+HOST) and right cluster (AUTH..LAST).
    flex_gap: usize,
}

impl Columns {
    /// Add ~10% breathing room to a content-measured column width.
    /// Returns 0 for 0-width columns (no content = no column).
    fn padded(w: usize) -> usize {
        if w == 0 { 0 } else { w + w / 10 + 1 }
    }

    #[allow(clippy::too_many_arguments)]
    fn compute(
        alias_w: usize,
        host_w: usize,
        tags_w: usize,
        tunnel_w: usize,
        auth_w: usize,
        has_ping: bool,
        history_w: usize,
        content: usize,
    ) -> Self {
        // All columns get ~110% of their content width for breathing room.
        // Columns are capped — they never grow beyond content needs.
        let alias = Self::padded(alias_w).clamp(8, 32);
        let mut host = Self::padded(host_w).max(HOST_MIN);
        let mut tags = if tags_w > 0 {
            Self::padded(tags_w).max(4)
        } else {
            0
        };
        let mut tunnel = if tunnel_w > 0 {
            Self::padded(tunnel_w).max(6)
        } else {
            0
        };
        let mut auth = if auth_w > 0 {
            Self::padded(auth_w).max(4)
        } else {
            0
        };
        let mut show_ping = has_ping;
        let mut history = if history_w > 0 {
            Self::padded(history_w).max(4)
        } else {
            0
        };

        // Fixed gap between columns within a cluster
        let gap: usize = if content >= 120 { 3 } else { 2 };

        // Total width of the right cluster (AUTH, TUNNEL, PING, TAGS, LAST + gaps)
        let right_cluster =
            |tags: usize, tunnel: usize, auth: usize, ping: bool, history: usize| -> usize {
                let mut w = 0usize;
                let mut n = 0usize;
                if auth > 0 {
                    w += auth;
                    n += 1;
                }
                if tunnel > 0 {
                    w += tunnel;
                    n += 1;
                }
                if ping {
                    w += 4;
                    n += 1;
                }
                if tags > 0 {
                    w += tags;
                    n += 1;
                }
                if history > 0 {
                    w += history;
                    n += 1;
                }
                let gaps = if n > 1 { (n - 1) * gap } else { 0 };
                w + gaps
            };

        // Left cluster: highlight_symbol(1) + marker + NAME + gap + HOST
        let left = MARKER_WIDTH + 1 + alias + gap + host;

        // Total with minimum flex_gap = gap
        let mut rw = right_cluster(tags, tunnel, auth, show_ping, history);
        let min_total = left + gap + rw;

        // Hide right-cluster columns by priority: AUTH → TUNNEL → LAST → PING → TAGS
        if min_total > content && auth > 0 {
            auth = 0;
            rw = right_cluster(tags, tunnel, auth, show_ping, history);
        }
        if left + gap + rw > content && tunnel > 0 {
            tunnel = 0;
            rw = right_cluster(tags, tunnel, auth, show_ping, history);
        }
        if left + gap + rw > content && history > 0 {
            history = 0;
            rw = right_cluster(tags, tunnel, auth, show_ping, history);
        }
        if left + gap + rw > content && show_ping {
            show_ping = false;
            rw = right_cluster(tags, tunnel, auth, show_ping, history);
        }
        if left + gap + rw > content && tags > 0 {
            tags = 0;
            rw = right_cluster(tags, tunnel, auth, show_ping, history);
        }

        // Still too wide: shrink HOST
        let needed = MARKER_WIDTH + 1 + alias + gap + host + gap + rw;
        if needed > content {
            host = host.saturating_sub(needed - content);
        }
        host = host.max(HOST_MIN);

        // Flex gap: remaining space between left and right clusters
        let left_final = MARKER_WIDTH + 1 + alias + gap + host;
        let flex_gap = if rw > 0 {
            content.saturating_sub(left_final + rw)
        } else {
            0
        };

        Columns {
            alias,
            host,
            tags,
            tunnel,
            auth,
            show_ping,
            history,
            gap,
            flex_gap,
        }
    }
}

/// Short label for a password source suitable for column display.
fn password_label(source: &str) -> &'static str {
    if source == "keychain" {
        "keychain"
    } else if source.starts_with("op://") {
        "1password"
    } else if source.starts_with("bw:") {
        "bitwarden"
    } else if source.starts_with("pass:") {
        "pass"
    } else if source.starts_with("vault:") {
        "vault"
    } else {
        "custom"
    }
}

/// Derive a short auth label from identity_file path or askpass source.
/// Only shows explicitly configured auth. No implicit/default key detection.
fn auth_label(host: &crate::ssh_config::model::HostEntry) -> String {
    if !host.identity_file.is_empty() {
        let path = std::path::Path::new(&host.identity_file);
        path.file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| host.identity_file.clone())
    } else if let Some(ref source) = host.askpass {
        password_label(source).to_string()
    } else {
        String::new()
    }
}

/// Compute the display width of the composite host label (user@hostname:port).
fn composite_host_width(host: &crate::ssh_config::model::HostEntry) -> usize {
    composite_host_label(host).width()
}

/// Build composite host label: user@hostname:port (only showing non-default parts).
fn composite_host_label(host: &crate::ssh_config::model::HostEntry) -> String {
    let mut s = String::new();
    if !host.user.is_empty() {
        s.push_str(&host.user);
        s.push('@');
    }
    s.push_str(&host.hostname);
    if host.port != 22 {
        s.push(':');
        s.push_str(&host.port.to_string());
    }
    s
}

/// Build a short tunnel summary for a host, e.g. "L:5432" or "L:5432 +1".
fn tunnel_summary(elements: &[ConfigElement], alias: &str) -> String {
    let rules = collect_tunnel_labels(elements, alias);
    if rules.is_empty() {
        return String::new();
    }
    if rules.len() == 1 {
        rules[0].clone()
    } else {
        format!("{} +{}", rules[0], rules.len() - 1)
    }
}

fn collect_tunnel_labels(elements: &[ConfigElement], alias: &str) -> Vec<String> {
    for element in elements {
        match element {
            ConfigElement::HostBlock(block) if block.host_pattern == alias => {
                return block
                    .directives
                    .iter()
                    .filter(|d| !d.is_non_directive)
                    .filter_map(|d| {
                        let prefix = match d.key.to_lowercase().as_str() {
                            "localforward" => "L",
                            "remoteforward" => "R",
                            "dynamicforward" => "D",
                            _ => return None,
                        };
                        // Extract just the bind port (first token)
                        let port = d.value.split_whitespace().next().unwrap_or(&d.value);
                        Some(format!("{}:{}", prefix, port))
                    })
                    .collect();
            }
            ConfigElement::Include(include) => {
                for file in &include.resolved_files {
                    let result = collect_tunnel_labels(&file.elements, alias);
                    if !result.is_empty() {
                        return result;
                    }
                }
            }
            _ => {}
        }
    }
    Vec::new()
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();

    let is_searching = app.search.query.is_some();
    let is_tagging = app.tag_input.is_some();

    // Layout: host list + optional input bar + spacer + footer/status
    let chunks = if is_searching || is_tagging {
        Layout::vertical([
            Constraint::Min(5),    // Host list (maximized)
            Constraint::Length(1), // Search/tag bar
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Footer or status message
        ])
        .split(area)
    } else {
        Layout::vertical([
            Constraint::Min(5),    // Host list (maximized)
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Footer or status message
        ])
        .split(area)
    };

    let content_area = chunks[0];
    let target_detail =
        app.view_mode == ViewMode::Detailed && content_area.width >= DETAIL_MIN_WIDTH;
    let full_detail_width = if content_area.width >= 140 {
        46u16
    } else {
        40u16
    };

    // Calculate detail width: animated or instant.
    // Only animate when the terminal is wide enough for the detail panel.
    let detail_width = if content_area.width >= DETAIL_MIN_WIDTH {
        if let Some(progress) = app.detail_anim_progress() {
            (progress * full_detail_width as f32).round() as u16
        } else if target_detail {
            full_detail_width
        } else {
            0
        }
    } else {
        0
    };
    let use_detail = detail_width > 0;

    // Minimum width before we render detail content (border + 1 char padding)
    const DETAIL_RENDER_MIN: u16 = 8;

    let (list_area, detail_area) = if use_detail {
        let [left, right] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(detail_width)])
                .areas(content_area);
        (left, Some(right))
    } else {
        (content_area, None)
    };

    if is_searching {
        render_search_list(frame, app, list_area);
        render_search_bar(frame, app, chunks[1]);
        super::render_footer_with_status(frame, chunks[3], search_footer_spans(), app);
    } else if is_tagging {
        render_display_list(frame, app, list_area);
        render_tag_bar(frame, app, chunks[1]);
        super::render_footer_with_status(frame, chunks[3], tag_footer_spans(), app);
    } else {
        render_display_list(frame, app, list_area);
        let spans = if app.is_pattern_selected() {
            pattern_footer_spans(target_detail)
        } else {
            footer_spans(
                target_detail,
                app.multi_select.len(),
                app.hosts.iter().filter(|h| h.stale.is_some()).count(),
            )
        };
        super::render_footer_with_help(frame, chunks[2], spans, app);
    }

    if let Some(detail) = detail_area {
        if detail.width >= DETAIL_RENDER_MIN {
            super::detail_panel::render(frame, app, detail);
        } else {
            // During animation: render empty bordered area
            let block = ratatui::widgets::Block::bordered()
                .border_type(ratatui::widgets::BorderType::Rounded)
                .border_style(theme::border());
            frame.render_widget(block, detail);
        }
    }
}

fn render_display_list(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Build multi-span title: brand badge + position counter
    let host_count = app.hosts.len() + app.patterns.len();
    let title = if host_count == 0 {
        Line::from(Span::styled(" purple. ", theme::brand_badge()))
    } else {
        let pos = if let Some(sel) = app.ui.list_state.selected() {
            app.display_list
                .get(..=sel)
                .map(|slice| {
                    slice
                        .iter()
                        .filter(|item| {
                            matches!(
                                item,
                                HostListItem::Host { .. } | HostListItem::Pattern { .. }
                            )
                        })
                        .count()
                })
                .unwrap_or(0)
        } else {
            0
        };
        let mut title_spans = vec![
            Span::styled(" purple. ", theme::brand_badge()),
            Span::raw(format!(" {}/{} ", pos, host_count)),
        ];
        if app.tag_input.is_some() {
            title_spans.push(Span::styled(" TAGGING ", theme::brand_badge()));
        } else if !app.multi_select.is_empty() {
            title_spans.push(Span::styled(
                format!(" {} SELECTED ", app.multi_select.len()),
                theme::brand_badge(),
            ));
        }
        Line::from(title_spans)
    };

    let update_title = app.update_available.as_ref().map(|ver| {
        let label = build_update_label(
            ver,
            app.update_headline.as_deref(),
            app.update_hint,
            area.width,
        );
        Line::from(Span::styled(label, theme::update_badge()))
    });

    let url_label = Line::from(Span::styled(" getpurple.sh ", theme::muted()));

    if app.hosts.is_empty() {
        let mut block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(title)
            .title_bottom(url_label.clone().right_aligned())
            .border_style(theme::border());
        if let Some(update) = update_title {
            block = block.title_top(update.right_aligned());
        }
        let msg = if matches!(app.screen, app::Screen::Welcome { .. }) {
            ""
        } else {
            "  It's quiet in here... Press 'a' to add a host or 'S' for cloud sync."
        };
        let empty_msg = Paragraph::new(msg).style(theme::muted()).block(block);
        frame.render_widget(empty_msg, area);
        return;
    }

    // Build block and render border separately for column header
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .title_bottom(url_label.right_aligned())
        .border_style(theme::border());
    if let Some(update) = update_title {
        block = block.title_top(update.right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Populate tunnel summaries cache if empty (invalidated on config reload)
    if app.tunnel_summaries_cache.is_empty() && app.hosts.iter().any(|h| h.tunnel_count > 0) {
        for h in &app.hosts {
            if h.tunnel_count > 0 {
                let summary = tunnel_summary(&app.config.elements, &h.alias);
                app.tunnel_summaries_cache.insert(h.alias.clone(), summary);
            }
        }
    }
    let tunnel_summaries = &app.tunnel_summaries_cache;

    // Compute column layout
    let content_width = (inner.width as usize).saturating_sub(2); // -1 highlight, -1 right margin
    let alias_w = app.hosts.iter().map(|h| h.alias.width()).max().unwrap_or(8);
    let host_w = app
        .hosts
        .iter()
        .map(composite_host_width)
        .max()
        .unwrap_or(12);
    let tags_w = app.hosts.iter().map(host_tags_width).max().unwrap_or(0);
    let tunnel_w = tunnel_summaries
        .values()
        .map(|s| s.width())
        .max()
        .unwrap_or(0);
    let auth_w = app
        .hosts
        .iter()
        .map(|h| auth_label(h).width())
        .max()
        .unwrap_or(0);
    let has_ping = !app.ping_status.is_empty();
    let history_w = app
        .hosts
        .iter()
        .filter_map(|h| app.history.entries.get(&h.alias))
        .map(|e| crate::history::ConnectionHistory::format_time_ago(e.last_connected))
        .filter(|s| !s.is_empty())
        .map(|s| s.width())
        .max()
        .unwrap_or(0);
    let cols = Columns::compute(
        alias_w,
        host_w,
        tags_w,
        tunnel_w,
        auth_w,
        has_ping,
        history_w,
        content_width,
    );

    // Column header + underline + list body
    let [header_area, underline_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    render_header(frame, header_area, &cols, app.sort_mode);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(underline_area.width as usize),
            theme::muted(),
        )),
        underline_area,
    );

    // Use pre-computed group host counts (computed before collapse filtering)
    let group_counts = &app.group_host_counts;

    let mut items: Vec<ListItem> = Vec::new();
    for item in &app.display_list {
        match item {
            HostListItem::GroupHeader(text) => {
                let upper = text.to_uppercase();
                let count = group_counts.get(text.as_str()).copied().unwrap_or(0);
                let collapsed = app.collapsed_groups.contains(text);
                let arrow = if collapsed { "\u{25B6} " } else { "\u{25BC} " };
                let name_part = format!("{}{}  ", arrow, upper);
                let count_part = format!("{} ", count);
                let label_width = name_part.width() + count_part.width();
                let fill = content_width.saturating_sub(label_width);
                let line = Line::from(vec![
                    Span::styled(name_part, theme::bold()),
                    Span::styled(count_part, theme::muted()),
                    Span::styled("─".repeat(fill), theme::muted()),
                ]);
                items.push(ListItem::new(line));
            }
            HostListItem::Host { index } => {
                if let Some(host) = app.hosts.get(*index) {
                    let tunnel_active = app.active_tunnels.contains_key(&host.alias);
                    let list_item = build_host_item(
                        host,
                        &app.ping_status,
                        &app.history,
                        tunnel_summaries,
                        tunnel_active,
                        None,
                        &cols,
                        app.multi_select.contains(index),
                    );
                    items.push(list_item);
                } else {
                    items.push(ListItem::new(Line::from(Span::raw(""))));
                }
            }
            HostListItem::Pattern { index } => {
                if let Some(pattern) = app.patterns.get(*index) {
                    items.push(build_pattern_item(pattern, &cols));
                } else {
                    items.push(ListItem::new(Line::from(Span::raw(""))));
                }
            }
        }
    }

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol("\u{258C}");

    frame.render_stateful_widget(list, list_area, &mut app.ui.list_state);
}

fn render_search_list(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let total_results =
        app.search.filtered_indices.len() + app.search.filtered_pattern_indices.len();
    let total = app.hosts.len() + app.patterns.len();
    let title = Line::from(vec![
        Span::styled(" purple. ", theme::brand_badge()),
        Span::raw(format!(" search: {}/{} ", total_results, total)),
    ]);

    let update_title = app.update_available.as_ref().map(|ver| {
        let label = build_update_label(
            ver,
            app.update_headline.as_deref(),
            app.update_hint,
            area.width,
        );
        Line::from(Span::styled(label, theme::update_badge()))
    });

    let url_label = Line::from(Span::styled(" getpurple.sh ", theme::muted()));

    if app.search.filtered_indices.is_empty() && app.search.filtered_pattern_indices.is_empty() {
        let mut block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(title)
            .title_bottom(url_label.clone().right_aligned())
            .border_style(theme::border_search());
        if let Some(update) = update_title {
            block = block.title_top(update.right_aligned());
        }
        let empty_msg = Paragraph::new("  No matches. Try a different search.")
            .style(theme::muted())
            .block(block);
        frame.render_widget(empty_msg, area);
        return;
    }

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .title_bottom(url_label.right_aligned())
        .border_style(theme::border_search());
    if let Some(update) = update_title {
        block = block.title_top(update.right_aligned());
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Populate tunnel summaries cache if empty (invalidated on config reload)
    if app.tunnel_summaries_cache.is_empty() && app.hosts.iter().any(|h| h.tunnel_count > 0) {
        for h in &app.hosts {
            if h.tunnel_count > 0 {
                let summary = tunnel_summary(&app.config.elements, &h.alias);
                app.tunnel_summaries_cache.insert(h.alias.clone(), summary);
            }
        }
    }
    let tunnel_summaries = &app.tunnel_summaries_cache;

    let content_width = (inner.width as usize).saturating_sub(2); // -1 highlight, -1 right margin
    let filtered_hosts = || {
        app.search
            .filtered_indices
            .iter()
            .filter_map(|&i| app.hosts.get(i))
    };
    let alias_w = filtered_hosts().map(|h| h.alias.width()).max().unwrap_or(8);
    let host_w = filtered_hosts()
        .map(composite_host_width)
        .max()
        .unwrap_or(12);
    let tags_w = filtered_hosts().map(host_tags_width).max().unwrap_or(0);
    let tunnel_w = filtered_hosts()
        .filter_map(|h| tunnel_summaries.get(&h.alias))
        .map(|s| s.width())
        .max()
        .unwrap_or(0);
    let auth_w = filtered_hosts()
        .map(|h| auth_label(h).width())
        .max()
        .unwrap_or(0);
    let has_ping = !app.ping_status.is_empty();
    let history_w = filtered_hosts()
        .filter_map(|h| app.history.entries.get(&h.alias))
        .map(|e| crate::history::ConnectionHistory::format_time_ago(e.last_connected))
        .filter(|s| !s.is_empty())
        .map(|s| s.width())
        .max()
        .unwrap_or(0);
    let cols = Columns::compute(
        alias_w,
        host_w,
        tags_w,
        tunnel_w,
        auth_w,
        has_ping,
        history_w,
        content_width,
    );

    let [header_area, underline_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    render_header(frame, header_area, &cols, app.sort_mode);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(underline_area.width as usize),
            theme::muted(),
        )),
        underline_area,
    );

    let query = app.search.query.as_deref();
    let mut items: Vec<ListItem> = Vec::new();
    for &idx in app.search.filtered_indices.iter() {
        if let Some(host) = app.hosts.get(idx) {
            let tunnel_active = app.active_tunnels.contains_key(&host.alias);
            let list_item = build_host_item(
                host,
                &app.ping_status,
                &app.history,
                tunnel_summaries,
                tunnel_active,
                query,
                &cols,
                app.multi_select.contains(&idx),
            );
            items.push(list_item);
        }
    }
    for &idx in app.search.filtered_pattern_indices.iter() {
        if let Some(pattern) = app.patterns.get(idx) {
            items.push(build_pattern_item(pattern, &cols));
        }
    }

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol("\u{258C}");

    frame.render_stateful_widget(list, list_area, &mut app.ui.list_state);
}

fn render_header(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    cols: &Columns,
    sort_mode: crate::app::SortMode,
) {
    use crate::app::SortMode;
    let style = theme::bold();
    let gap = " ".repeat(cols.gap);
    let flex = " ".repeat(cols.flex_gap);

    // Sort indicator: ▾ next to the active sort column
    let name_sort = matches!(sort_mode, SortMode::AlphaAlias);
    let host_sort = matches!(sort_mode, SortMode::AlphaHostname);
    let last_sort = matches!(sort_mode, SortMode::MostRecent | SortMode::Frecency);

    let mut spans = vec![
        Span::styled(
            format!(
                "{}{:<width$}",
                " ".repeat(MARKER_WIDTH + 1),
                if name_sort { "NAME \u{25BE}" } else { "NAME" },
                width = cols.alias
            ),
            style,
        ),
        Span::raw(gap.clone()),
        Span::styled(
            format!(
                "{:<width$}",
                if host_sort { "HOST \u{25BE}" } else { "HOST" },
                width = cols.host
            ),
            style,
        ),
    ];
    // Flex gap between left and right cluster
    if cols.flex_gap > 0 {
        spans.push(Span::raw(flex));
    }
    if cols.auth > 0 {
        spans.push(Span::styled(
            format!("{:<width$}", "AUTH", width = cols.auth),
            style,
        ));
        spans.push(Span::raw(gap.clone()));
    }
    if cols.tunnel > 0 {
        spans.push(Span::styled(
            format!("{:<width$}", "TUNNEL", width = cols.tunnel),
            style,
        ));
        spans.push(Span::raw(gap.clone()));
    }
    if cols.show_ping {
        spans.push(Span::styled("PING", style));
        spans.push(Span::raw(gap.clone()));
    }
    if cols.tags > 0 {
        spans.push(Span::styled(
            format!("{:<width$}", "TAGS", width = cols.tags),
            style,
        ));
        spans.push(Span::raw(gap.clone()));
    }
    if cols.history > 0 {
        spans.push(Span::styled(
            format!("{:>width$}", "LAST", width = cols.history),
            style,
        ));
        if last_sort {
            spans.push(Span::styled("\u{25BE}", style));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Compute the display width of a host's tags (including provider_tags, provider and source file).
fn host_tags_width(host: &crate::ssh_config::model::HostEntry) -> usize {
    let mut w = 0usize;
    for tag in host.provider_tags.iter().chain(host.tags.iter()) {
        if w > 0 {
            w += 1;
        } // space between tags
        w += 1 + tag.width(); // # + tag
    }
    if let Some(ref label) = host.provider {
        if w > 0 {
            w += 1;
        }
        w += 1 + label.width(); // # + label
    }
    if let Some(ref source) = host.source_file {
        let name = source
            .file_name()
            .map(|f| f.to_string_lossy().width())
            .unwrap_or(0);
        if name > 0 {
            if w > 0 {
                w += 1;
            }
            w += name + 2; // (filename)
        }
    }
    w
}

#[allow(clippy::too_many_arguments)]
fn build_host_item<'a>(
    host: &'a crate::ssh_config::model::HostEntry,
    ping_status: &'a std::collections::HashMap<String, PingStatus>,
    history: &'a crate::history::ConnectionHistory,
    tunnel_summaries: &'a std::collections::HashMap<String, String>,
    tunnel_active: bool,
    query: Option<&str>,
    cols: &Columns,
    multi_selected: bool,
) -> ListItem<'a> {
    let q = query.unwrap_or("");
    let gap = " ".repeat(cols.gap);

    // Determine which field matches for search highlighting
    let alias_matches = !q.is_empty() && app::contains_ci(&host.alias, q);
    let host_matches = !alias_matches
        && !q.is_empty()
        && (app::contains_ci(&host.hostname, q) || app::contains_ci(&host.user, q));

    let mut spans: Vec<Span> = Vec::new();

    // === NAME column (fixed width) ===
    let is_stale = host.stale.is_some();
    let alias_style = if alias_matches {
        theme::highlight_bold()
    } else if is_stale {
        theme::muted()
    } else {
        theme::bold()
    };
    let marker = if multi_selected { " \u{2713}" } else { "  " };
    let alias_truncated = super::truncate(&host.alias, cols.alias);
    spans.push(Span::styled(
        format!("{}{:<width$}", marker, alias_truncated, width = cols.alias),
        alias_style,
    ));
    spans.push(Span::raw(gap.clone()));

    // === HOST column (flex width): user@hostname:port composite ===
    let composite = composite_host_label(host);
    let composite_trunc = super::truncate(&composite, cols.host);
    let mut host_used = 0usize;

    // Render with dim user@ prefix and :port suffix, normal hostname
    if host_matches {
        // Entire composite highlighted when searching
        host_used = composite_trunc.width();
        spans.push(Span::styled(composite_trunc, theme::highlight_bold()));
    } else {
        // Split into styled parts: user@ (dim), hostname (normal), :port (dim)
        let has_user = !host.user.is_empty();
        let has_port = host.port != 22;
        let user_prefix = if has_user {
            format!("{}@", host.user)
        } else {
            String::new()
        };
        let port_suffix = if has_port {
            format!(":{}", host.port)
        } else {
            String::new()
        };

        let has_jump = !host.proxy_jump.is_empty();
        let jump_w = if has_jump { 2 } else { 0 }; // " →"
        let available = cols.host;
        let prefix_w = user_prefix.width();
        let suffix_w = port_suffix.width();
        let hostname_budget = available.saturating_sub(prefix_w + suffix_w + jump_w);

        if hostname_budget >= 4 || !has_user {
            // Enough room: show user@, truncated hostname, :port, →
            if has_user {
                spans.push(Span::styled(user_prefix.clone(), theme::muted()));
                host_used += prefix_w;
            }
            let hostname_trunc = super::truncate(&host.hostname, hostname_budget);
            host_used += hostname_trunc.width();
            let hostname_style = if is_stale {
                theme::muted()
            } else {
                Style::default()
            };
            spans.push(Span::styled(hostname_trunc, hostname_style));
            if has_port {
                spans.push(Span::styled(port_suffix, theme::muted()));
                host_used += suffix_w;
            }
            if has_jump {
                spans.push(Span::styled(" \u{2192}", theme::muted()));
                host_used += 2;
            }
        } else {
            // Very tight: just truncate the whole composite
            host_used = composite_trunc.width();
            let style = if is_stale {
                theme::muted()
            } else {
                Style::default()
            };
            spans.push(Span::styled(composite_trunc, style));
        }
    }

    let host_pad = cols.host.saturating_sub(host_used);
    if host_pad > 0 {
        spans.push(Span::raw(" ".repeat(host_pad)));
    }

    // === Flex gap between left cluster (NAME+HOST) and right cluster ===
    if cols.flex_gap > 0 {
        spans.push(Span::raw(" ".repeat(cols.flex_gap)));
    }

    // === AUTH column (SSH key or password source) ===
    if cols.auth > 0 {
        let label = auth_label(host);
        if !label.is_empty() {
            let style = theme::muted();
            spans.push(Span::styled(
                format!("{:<width$}", label, width = cols.auth),
                style,
            ));
        } else {
            spans.push(Span::raw(" ".repeat(cols.auth)));
        }
        spans.push(Span::raw(gap.clone()));
    }

    // === TUNNEL column ===
    if cols.tunnel > 0 {
        if let Some(summary) = tunnel_summaries.get(&host.alias) {
            let style = if tunnel_active {
                theme::success()
            } else {
                theme::muted()
            };
            spans.push(Span::styled(
                format!("{:<width$}", summary, width = cols.tunnel),
                style,
            ));
        } else {
            spans.push(Span::raw(" ".repeat(cols.tunnel)));
        }
        spans.push(Span::raw(gap.clone()));
    }

    // === PING column ===
    if cols.show_ping {
        if let Some(status) = ping_status.get(&host.alias) {
            let (indicator, style) = match status {
                PingStatus::Checking => ("..", theme::muted()),
                PingStatus::Reachable => ("ok", theme::success()),
                PingStatus::Unreachable => ("--", theme::error()),
                PingStatus::Skipped => ("??", theme::muted()),
            };
            spans.push(Span::raw(" "));
            spans.push(Span::styled(indicator, style));
            spans.push(Span::raw(" "));
        } else {
            spans.push(Span::raw("    "));
        }
        spans.push(Span::raw(gap.clone()));
    }

    // === TAGS column (fixed width, +N overflow) ===
    if cols.tags > 0 {
        let tag_matches = !q.is_empty() && !alias_matches && !host_matches;
        build_tag_column(&mut spans, host, tag_matches, q, cols.tags);
        if cols.history > 0 {
            spans.push(Span::raw(gap.clone()));
        }
    }

    // === LAST column (right-aligned) ===
    if cols.history > 0 {
        if let Some(entry) = history.entries.get(&host.alias) {
            let ago = crate::history::ConnectionHistory::format_time_ago(entry.last_connected);
            if !ago.is_empty() {
                spans.push(Span::styled(
                    format!("{:>width$}", ago, width = cols.history),
                    theme::muted(),
                ));
            } else {
                spans.push(Span::styled(
                    format!("{:>width$}", "-", width = cols.history),
                    theme::muted(),
                ));
            }
        } else {
            spans.push(Span::styled(
                format!("{:>width$}", "-", width = cols.history),
                theme::muted(),
            ));
        }
    }

    ListItem::new(Line::from(spans))
}

fn build_pattern_item<'a>(
    pattern: &'a crate::ssh_config::model::PatternEntry,
    cols: &Columns,
) -> ListItem<'a> {
    let gap = " ".repeat(cols.gap);
    let mut spans: Vec<Span> = Vec::new();

    // NAME column: * prefix in accent, pattern text in muted
    let prefix = "* ";
    let prefix_w = UnicodeWidthStr::width(prefix);
    let alias_budget = cols.alias.saturating_sub(prefix_w);
    let pattern_trunc = super::truncate(&pattern.pattern, alias_budget);

    spans.push(Span::styled(format!("  {}", prefix), theme::accent()));
    spans.push(Span::styled(
        format!("{:<width$}", pattern_trunc, width = alias_budget),
        theme::muted(),
    ));
    spans.push(Span::raw(gap.clone()));

    // HOST column: hostname if present, else empty
    let host_display = if !pattern.hostname.is_empty() {
        super::truncate(&pattern.hostname, cols.host)
    } else {
        String::new()
    };
    let host_used = UnicodeWidthStr::width(host_display.as_str());
    if !host_display.is_empty() {
        spans.push(Span::styled(host_display, theme::muted()));
    }
    let host_pad = cols.host.saturating_sub(host_used);
    if host_pad > 0 {
        spans.push(Span::raw(" ".repeat(host_pad)));
    }

    // AUTH column: identity file if present
    if cols.flex_gap > 0 {
        spans.push(Span::raw(" ".repeat(cols.flex_gap)));
    }
    if cols.auth > 0 {
        let auth_display = if !pattern.identity_file.is_empty() {
            let path = std::path::Path::new(&pattern.identity_file);
            let label = path
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| pattern.identity_file.clone());
            super::truncate(&label, cols.auth)
        } else {
            String::new()
        };
        let auth_used = UnicodeWidthStr::width(auth_display.as_str());
        if !auth_display.is_empty() {
            spans.push(Span::styled(auth_display, theme::muted()));
        }
        let auth_pad = cols.auth.saturating_sub(auth_used);
        if auth_pad > 0 {
            spans.push(Span::raw(" ".repeat(auth_pad)));
        }
        spans.push(Span::raw(gap.clone()));
    }
    if cols.tunnel > 0 {
        spans.push(Span::raw(" ".repeat(cols.tunnel)));
        spans.push(Span::raw(gap.clone()));
    }
    if cols.show_ping {
        spans.push(Span::raw("    "));
        spans.push(Span::raw(gap.clone()));
    }
    if cols.tags > 0 {
        build_pattern_tag_column(&mut spans, pattern, cols.tags);
        if cols.history > 0 {
            spans.push(Span::raw(gap));
        }
    }
    if cols.history > 0 {
        spans.push(Span::raw(" ".repeat(cols.history)));
    }

    ListItem::new(Line::from(spans))
}

/// Render styled tags into spans within a fixed column width, with +N overflow.
fn render_tag_spans(spans: &mut Vec<Span<'_>>, all_tags: &[(String, Style)], width: usize) {
    let mut used = 0usize;
    let mut shown = 0usize;
    for (i, (tag, style)) in all_tags.iter().enumerate() {
        let sep = if shown > 0 { 1 } else { 0 };
        let tag_w = tag.width();
        let remaining = all_tags.len() - i - 1;
        let overflow_count = all_tags.len() - i;
        let overflow_reserve = if remaining > 0 {
            format!(" +{}", overflow_count).width()
        } else {
            0
        };

        if used + sep + tag_w <= width
            && (remaining == 0 || used + sep + tag_w + overflow_reserve <= width)
        {
            if shown > 0 {
                spans.push(Span::raw(" "));
                used += 1;
            }
            spans.push(Span::styled(tag.clone(), *style));
            used += tag_w;
            shown += 1;
        } else {
            let count = all_tags.len() - i;
            let overflow = if shown > 0 {
                format!(" +{}", count)
            } else {
                format!("+{}", count)
            };
            spans.push(Span::styled(overflow.clone(), theme::muted()));
            used += overflow.width();
            break;
        }
    }

    let pad = width.saturating_sub(used);
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    }
}

/// Build tag spans for a pattern entry.
fn build_pattern_tag_column(
    spans: &mut Vec<Span<'_>>,
    pattern: &crate::ssh_config::model::PatternEntry,
    width: usize,
) {
    let all_tags: Vec<(String, Style)> = pattern
        .tags
        .iter()
        .map(|t| (format!("#{}", t), theme::muted()))
        .collect();
    render_tag_spans(spans, &all_tags, width);
}

/// Build tag spans that fit within a fixed column width, with +N overflow.
fn build_tag_column(
    spans: &mut Vec<Span<'_>>,
    host: &crate::ssh_config::model::HostEntry,
    tag_matches: bool,
    query: &str,
    width: usize,
) {
    let mut all_tags: Vec<(String, Style)> = Vec::new();
    for tag in host.provider_tags.iter().chain(host.tags.iter()) {
        let style = if tag_matches && app::contains_ci(tag, query) {
            theme::highlight_bold()
        } else {
            theme::muted()
        };
        all_tags.push((format!("#{}", tag), style));
    }
    if let Some(ref label) = host.provider {
        let style = if tag_matches && app::contains_ci(label, query) {
            theme::highlight_bold()
        } else {
            theme::muted()
        };
        all_tags.push((format!("#{}", label), style));
    }
    if let Some(ref source) = host.source_file {
        if let Some(name) = source.file_name() {
            let s = name.to_string_lossy();
            if !s.is_empty() {
                all_tags.push((format!("({})", s), theme::muted()));
            }
        }
    }
    render_tag_spans(spans, &all_tags, width);
}

fn render_search_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let query = app.search.query.as_deref().unwrap_or("");
    let total = app.hosts.len() + app.patterns.len();
    let match_info = if query.is_empty() {
        String::new()
    } else {
        let count = app.search.filtered_indices.len() + app.search.filtered_pattern_indices.len();
        format!(" ({} of {})", count, total)
    };
    let search_line = Line::from(vec![
        Span::styled(" / ", theme::brand_badge()),
        Span::raw(" "),
        Span::raw(query),
        Span::styled("_", theme::accent()),
        Span::styled(match_info, theme::muted()),
    ]);
    frame.render_widget(Paragraph::new(search_line), area);
}

fn footer_spans(detail_active: bool, multi_count: usize, stale_count: usize) -> Vec<Span<'static>> {
    let view_label = if detail_active {
        " compact "
    } else {
        " detail "
    };
    let mut spans = vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" connect ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("/", theme::accent_bold()),
        Span::styled(" search ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("#", theme::accent_bold()),
        Span::styled(" tag ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("a", theme::accent_bold()),
        Span::styled(" add ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("e", theme::accent_bold()),
        Span::styled(" edit ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("d", theme::accent_bold()),
        Span::styled(" del ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("r", theme::accent_bold()),
        Span::styled(" run ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("f", theme::accent_bold()),
        Span::styled(" files ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("T", theme::accent_bold()),
        Span::styled(" tunnels ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("C", theme::accent_bold()),
        Span::styled(" containers ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("S", theme::accent_bold()),
        Span::styled(" sync ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("v", theme::accent_bold()),
        Span::styled(view_label, theme::muted()),
    ];
    if multi_count > 0 {
        spans.push(Span::styled("\u{2502} ", theme::muted()));
        spans.push(Span::styled(
            format!("{} selected ", multi_count),
            theme::accent_bold(),
        ));
    }
    if stale_count > 0 {
        spans.push(Span::styled("\u{2502} ", theme::muted()));
        spans.push(Span::styled("X", theme::accent_bold()));
        spans.push(Span::styled(
            format!(" purge {} stale ", stale_count),
            theme::muted(),
        ));
    }
    spans
}

fn pattern_footer_spans(detail_active: bool) -> Vec<Span<'static>> {
    let view_label = if detail_active {
        " compact "
    } else {
        " detail "
    };
    vec![
        Span::styled(" /", theme::accent_bold()),
        Span::styled(" search ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("#", theme::accent_bold()),
        Span::styled(" tag ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("A", theme::accent_bold()),
        Span::styled(" add pattern ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("e", theme::accent_bold()),
        Span::styled(" edit ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("d", theme::accent_bold()),
        Span::styled(" del ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("c", theme::accent_bold()),
        Span::styled(" clone ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("v", theme::accent_bold()),
        Span::styled(view_label, theme::muted()),
    ]
}

fn search_footer_spans<'a>() -> Vec<Span<'a>> {
    vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" connect ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Ctrl+E", theme::accent_bold()),
        Span::styled(" edit ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" cancel ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("tag:", theme::accent_bold()),
        Span::styled("fuzzy ", theme::muted()),
        Span::styled("tag=", theme::accent_bold()),
        Span::styled("exact", theme::muted()),
    ]
}

fn render_tag_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let input = app.tag_input.as_deref().unwrap_or("");
    let mut spans = vec![Span::styled(" tags: ", theme::accent_bold())];
    // Show read-only provider tags if present
    if let Some(host) = app.selected_host() {
        if !host.provider_tags.is_empty() {
            let ptags = host.provider_tags.join(", ");
            spans.push(Span::styled(format!("[{}] ", ptags), theme::muted()));
        }
    }
    spans.push(Span::raw(input));
    spans.push(Span::styled("_", theme::accent()));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn tag_footer_spans<'a>() -> Vec<Span<'a>> {
    vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" save ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" cancel ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("comma-separated", theme::muted()),
    ]
}

#[cfg(test)]
mod tests {
    use super::build_update_label;

    #[test]
    fn label_fits_fully() {
        let label = build_update_label("2.7.0", Some("New feature"), "purple update", 80);
        assert_eq!(label, " v2.7.0: New feature (run purple update) ");
    }

    #[test]
    fn label_no_headline() {
        let label = build_update_label("2.7.0", None, "purple update", 80);
        assert_eq!(label, " v2.7.0 available, run purple update ");
    }

    #[test]
    fn label_truncates_at_various_widths() {
        use unicode_width::UnicodeWidthStr;

        let hl = "Provider metadata uses provider-specific terminology (instance, vm_size, zone, location, image, specs)";
        let hint = "purple update";
        let full = " v2.7.0: Provider metadata uses provider-specific terminology (instance, vm_size, zone, location, image, specs) (run purple update) ";

        // Full label is 132 display columns; budget = width - 4
        assert_eq!(full.width(), 132);

        // 136+ cols: fits fully (budget >= 132)
        assert_eq!(build_update_label("2.7.0", Some(hl), hint, 136), full);

        // 80 cols: budget 76, headline truncated with ellipsis
        let label_80 = build_update_label("2.7.0", Some(hl), hint, 80);
        assert!(
            label_80.contains('\u{2026}'),
            "Should contain ellipsis: {}",
            label_80
        );
        assert!(label_80.contains("(run purple update)"));
        assert!(
            label_80.width() <= 76,
            "Should fit in budget: width={}",
            label_80.width()
        );

        // 60 cols: budget 56, headline truncated further
        let label_60 = build_update_label("2.7.0", Some(hl), hint, 60);
        assert!(label_60.contains('\u{2026}'));
        assert!(label_60.contains("(run purple update)"));
        assert!(
            label_60.width() <= 56,
            "Should fit in budget: width={}",
            label_60.width()
        );

        // Verify progressive truncation
        assert!(label_60.width() < label_80.width());

        // 30 cols: not enough room for headline, falls back to version-only
        assert_eq!(
            build_update_label("2.7.0", Some(hl), hint, 30),
            " v2.7.0 available, run purple update "
        );
    }

    #[test]
    fn label_falls_back_when_very_narrow() {
        let label = build_update_label("2.7.0", Some("Headline"), "purple update", 30);
        assert_eq!(label, " v2.7.0 available, run purple update ");
    }

    #[test]
    fn label_brew_hint() {
        let label = build_update_label(
            "2.7.0",
            Some("Fix"),
            "brew upgrade erickochen/purple/purple",
            80,
        );
        assert_eq!(
            label,
            " v2.7.0: Fix (run brew upgrade erickochen/purple/purple) "
        );
    }

    #[test]
    fn label_zero_width() {
        let label = build_update_label("2.7.0", Some("Headline"), "purple update", 0);
        assert_eq!(label, " v2.7.0 available, run purple update ");
    }

    // =========================================================================
    // Columns tests
    // =========================================================================

    use super::{Columns, HOST_MIN, MARKER_WIDTH, footer_spans};

    #[test]
    fn test_padded_zero() {
        assert_eq!(Columns::padded(0), 0);
    }

    #[test]
    fn test_padded_nonzero() {
        // padded(10) = 10 + 10/10 + 1 = 12
        assert_eq!(Columns::padded(10), 12);
    }

    #[test]
    fn test_columns_compute_hides_auth_first() {
        // Set up widths that are too wide for content area.
        // Auth should be hidden first, while tags remain visible.
        let cols = Columns::compute(
            10,    // alias_w
            20,    // host_w
            10,    // tags_w — should remain visible
            8,     // tunnel_w
            8,     // auth_w — should be hidden first
            false, // no ping
            6,     // history_w
            60,    // narrow content
        );
        assert_eq!(cols.auth, 0, "Auth should be hidden first when too narrow");
        assert!(
            cols.tags > 0,
            "Tags should still be present after auth is hidden"
        );
    }

    #[test]
    fn test_columns_compute_flex_gap() {
        let cols = Columns::compute(
            10,    // alias_w
            15,    // host_w
            8,     // tags_w
            6,     // tunnel_w
            6,     // auth_w
            false, // no ping
            5,     // history_w
            200,   // wide content
        );
        assert!(
            cols.flex_gap > 0,
            "flex_gap should be positive with wide content"
        );
        // Total consumed should not exceed content width
        let gap = if 200 >= 120 { 3 } else { 2 };
        let left = MARKER_WIDTH + 1 + cols.alias + gap + cols.host;
        let mut right = 0;
        if cols.auth > 0 {
            right += cols.auth;
        }
        if cols.tunnel > 0 {
            right += cols.tunnel;
        }
        if cols.tags > 0 {
            right += cols.tags;
        }
        if cols.history > 0 {
            right += cols.history;
        }
        // flex_gap fills the remaining space
        assert_eq!(
            cols.flex_gap,
            200usize.saturating_sub(left + right + (3 * 3))
        ); // approximate
    }

    #[test]
    fn test_columns_compute_host_shrinks() {
        // Very narrow content: host shrinks but stays >= HOST_MIN
        let cols = Columns::compute(
            8,     // alias_w
            30,    // host_w — should shrink
            0,     // no tags
            0,     // no tunnel
            0,     // no auth
            false, // no ping
            0,     // no history
            30,    // very narrow
        );
        assert!(
            cols.host >= HOST_MIN,
            "Host should stay >= HOST_MIN ({}), got {}",
            HOST_MIN,
            cols.host
        );
    }

    #[test]
    fn test_footer_spans_with_grouping_no_indicator() {
        // "grouped" indicator was removed (redundant with status bar)
        let spans = footer_spans(false, 0, 0);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            !text.contains("grouped"),
            "Footer should NOT contain 'grouped' indicator, got: {}",
            text
        );
    }

    #[test]
    fn test_footer_spans_with_stale_hosts() {
        let spans = footer_spans(false, 0, 5);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("X"));
        assert!(text.contains("purge 5 stale"));
    }

    #[test]
    fn test_footer_spans_no_stale() {
        let spans = footer_spans(false, 0, 0);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert!(!text.contains("stale"));
        assert!(!text.contains("purge"));
    }

    #[test]
    fn test_footer_spans_stale_single() {
        let spans = footer_spans(false, 0, 1);
        let text: String = spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("purge 1 stale"));
    }

    #[test]
    fn layout_has_spacer_between_list_and_footer() {
        use ratatui::layout::{Constraint, Layout, Rect};
        let area = Rect::new(0, 0, 120, 40);
        let chunks = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(1), // spacer
            Constraint::Length(1), // footer
        ])
        .split(area);
        assert_eq!(chunks[1].height, 1);
        assert_eq!(chunks[2].height, 1);
        assert!(chunks[2].y > chunks[0].y + chunks[0].height);
    }

    #[test]
    fn layout_with_search_has_spacer() {
        use ratatui::layout::{Constraint, Layout, Rect};
        let area = Rect::new(0, 0, 120, 40);
        let chunks = Layout::vertical([
            Constraint::Min(5),
            Constraint::Length(1), // search bar
            Constraint::Length(1), // spacer
            Constraint::Length(1), // footer
        ])
        .split(area);
        assert_eq!(chunks[2].height, 1);
        assert_eq!(chunks[3].height, 1);
        assert!(chunks[3].y > chunks[0].y + chunks[0].height);
    }

    // =========================================================================
    // Column hide priority tests
    // =========================================================================

    #[test]
    fn columns_hide_full_priority_chain() {
        // Wide enough for everything
        let cols_wide = Columns::compute(10, 15, 8, 6, 6, true, 5, 200);
        assert!(cols_wide.auth > 0, "auth visible at 200");
        assert!(cols_wide.tunnel > 0, "tunnel visible at 200");
        assert!(cols_wide.history > 0, "history visible at 200");
        assert!(cols_wide.show_ping, "ping visible at 200");
        assert!(cols_wide.tags > 0, "tags visible at 200");

        // Progressively narrower: AUTH hides first
        let cols_no_auth = Columns::compute(10, 15, 8, 6, 6, true, 5, 70);
        assert_eq!(cols_no_auth.auth, 0, "auth should hide first");

        // Even narrower: TUNNEL hides next
        let cols_no_tunnel = Columns::compute(10, 15, 8, 6, 6, true, 5, 55);
        assert_eq!(cols_no_tunnel.auth, 0, "auth still hidden");
        assert_eq!(cols_no_tunnel.tunnel, 0, "tunnel should hide second");

        // Narrower still: LAST (history) hides next
        let cols_no_history = Columns::compute(10, 15, 8, 6, 6, true, 5, 48);
        assert_eq!(cols_no_history.auth, 0);
        assert_eq!(cols_no_history.tunnel, 0);
        assert_eq!(cols_no_history.history, 0, "history should hide third");

        // Very narrow: PING hides next
        let cols_no_ping = Columns::compute(10, 15, 8, 6, 6, true, 5, 42);
        assert_eq!(cols_no_ping.auth, 0);
        assert_eq!(cols_no_ping.tunnel, 0);
        assert_eq!(cols_no_ping.history, 0);
        assert!(!cols_no_ping.show_ping, "ping should hide fourth");

        // Extremely narrow: TAGS hides last
        let cols_no_tags = Columns::compute(10, 15, 8, 6, 6, true, 5, 35);
        assert_eq!(cols_no_tags.auth, 0);
        assert_eq!(cols_no_tags.tunnel, 0);
        assert_eq!(cols_no_tags.history, 0);
        assert!(!cols_no_tags.show_ping);
        assert_eq!(cols_no_tags.tags, 0, "tags should hide last");
    }
}
