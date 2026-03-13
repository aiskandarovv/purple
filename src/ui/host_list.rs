use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, List, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{self, App, HostListItem, PingStatus, SortMode, ViewMode};
use crate::ssh_config::model::ConfigElement;

/// Minimum terminal width to show the detail panel in detailed view mode.
const DETAIL_MIN_WIDTH: u16 = 90;
const HOST_MIN: usize = 12;

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
    content: usize,
}

impl Columns {
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
        let alias = alias_w.clamp(8, 24);
        let host_content = host_w.max(HOST_MIN);
        let mut tags = tags_w;
        let mut tunnel = tunnel_w;
        let mut auth = auth_w;
        let mut show_ping = has_ping;
        let mut history = history_w;

        // Dynamic gap: more breathing room on wider terminals
        let base_gap = if content >= 160 { 7 } else if content >= 130 { 6 } else if content >= 100 { 5 } else { 4 };

        // Sum of all non-HOST fixed columns + gaps
        let fixed_rest = |gap: usize, tags: usize, tunnel: usize, auth: usize, ping: bool, history: usize| -> usize {
            let mut n = 2usize; // NAME + HOST always present
            let mut w = alias;
            if tags > 0 { w += tags; n += 1; }
            if tunnel > 0 { w += tunnel; n += 1; }
            if auth > 0 { w += auth; n += 1; }
            if ping { w += 4; n += 1; }
            if history > 0 { w += history; n += 1; }
            1 + w + (n - 1) * gap // 1 for marker
        };

        // HOST gets remaining space: at least content width, absorbs extra
        let calc_host = |gap: usize, tags: usize, tunnel: usize, auth: usize, ping: bool, history: usize| -> usize {
            content.saturating_sub(fixed_rest(gap, tags, tunnel, auth, ping, history))
        };

        let mut host = calc_host(base_gap, tags, tunnel, auth, show_ping, history);

        // Hide columns by priority until HOST >= content width
        if host < host_content && history > 0 {
            history = 0;
            host = calc_host(base_gap, tags, tunnel, auth, show_ping, history);
        }
        if host < host_content && tags > 0 {
            tags = 0;
            host = calc_host(base_gap, tags, tunnel, auth, show_ping, history);
        }
        if host < host_content && show_ping {
            show_ping = false;
            host = calc_host(base_gap, tags, tunnel, auth, show_ping, history);
        }
        if host < host_content && auth > 0 {
            auth = 0;
            host = calc_host(base_gap, tags, tunnel, auth, show_ping, history);
        }
        if host < host_content && tunnel > 0 {
            tunnel = 0;
            host = calc_host(base_gap, tags, tunnel, auth, show_ping, history);
        }
        host = host.max(HOST_MIN);

        Columns { alias, host, tags, tunnel, auth, show_ping, history, gap: base_gap, content }
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
/// Priority: key file name > password source. Shows the most relevant auth method.
fn auth_label(host: &crate::ssh_config::model::HostEntry) -> String {
    if !host.identity_file.is_empty() {
        // Extract filename from path like ~/.ssh/id_ed25519
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

    // Layout: host list + optional input bar + footer/status
    let chunks = if is_searching || is_tagging {
        Layout::vertical([
            Constraint::Min(5),   // Host list (maximized)
            Constraint::Length(1), // Search/tag bar
            Constraint::Length(1), // Footer or status message
        ])
        .split(area)
    } else {
        Layout::vertical([
            Constraint::Min(5),   // Host list (maximized)
            Constraint::Length(1), // Footer or status message
        ])
        .split(area)
    };

    let content_area = chunks[0];
    let use_detail =
        app.view_mode == ViewMode::Detailed && content_area.width >= DETAIL_MIN_WIDTH;

    let (list_area, detail_area) = if use_detail {
        let detail_width = if content_area.width >= 140 { 42 } else { 36 };
        let [left, right] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(detail_width),
        ])
        .areas(content_area);
        (left, Some(right))
    } else {
        (content_area, None)
    };

    if is_searching {
        render_search_list(frame, app, list_area);
        render_search_bar(frame, app, chunks[1]);
        super::render_footer_with_status(frame, chunks[2], search_footer_spans(), app);
    } else if is_tagging {
        render_display_list(frame, app, list_area);
        render_tag_bar(frame, app, chunks[1]);
        super::render_footer_with_status(frame, chunks[2], tag_footer_spans(), app);
    } else {
        render_display_list(frame, app, list_area);
        super::render_footer_with_status(frame, chunks[1], footer_spans(use_detail, app.multi_select.len()), app);
    }

    if let Some(detail) = detail_area {
        super::detail_panel::render(frame, app, detail);
    }
}

fn render_display_list(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    // Build multi-span title: brand badge + position counter
    let host_count = app.hosts.len();
    let title = if host_count == 0 {
        Line::from(Span::styled(" purple. ", theme::brand_badge()))
    } else {
        let pos = if let Some(sel) = app.ui.list_state.selected() {
            app.display_list.get(..=sel)
                .map(|slice| slice.iter().filter(|item| matches!(item, HostListItem::Host { .. })).count())
                .unwrap_or(0)
        } else {
            0
        };
        let mut spans = vec![
            Span::styled(" purple. ", theme::brand_badge()),
            Span::raw(format!(" {}/{} ", pos, host_count)),
        ];
        if app.sort_mode != SortMode::Original || app.group_by_provider {
            let mut label = String::new();
            if app.sort_mode != SortMode::Original {
                label.push_str(app.sort_mode.label());
            }
            if app.group_by_provider {
                if !label.is_empty() {
                    label.push_str(", ");
                }
                label.push_str("grouped");
            }
            spans.push(Span::raw(format!("({}) ", label)));
        }
        Line::from(spans)
    };

    let update_title = app.update_available.as_ref().map(|ver| {
        Line::from(Span::styled(
            format!(" v{} available — run '{}' ", ver, app.update_hint),
            theme::update_badge(),
        ))
    });

    if app.hosts.is_empty() {
        let mut block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(title)
            .border_style(theme::border());
        if let Some(update) = update_title {
            block = block.title_top(update.right_aligned());
        }
        let empty_msg =
            Paragraph::new("  It's quiet in here... Press 'a' to add a host or 'S' for cloud sync.")
                .style(theme::muted())
                .block(block);
        frame.render_widget(empty_msg, area);
        return;
    }

    // Build block and render border separately for column header
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
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
    let content_width = (inner.width as usize).saturating_sub(1);
    let alias_w = app.hosts.iter().map(|h| h.alias.width()).max().unwrap_or(8);
    let host_w = app.hosts.iter().map(composite_host_width).max().unwrap_or(12);
    let tags_w = app.hosts.iter().map(host_tags_width).max().unwrap_or(0);
    let tunnel_w = tunnel_summaries.values().map(|s| s.width()).max().unwrap_or(0);
    let auth_w = app.hosts.iter()
        .map(|h| auth_label(h).width())
        .max()
        .unwrap_or(0);
    let has_ping = !app.ping_status.is_empty();
    let history_w = app.hosts.iter()
        .filter_map(|h| app.history.entries.get(&h.alias))
        .map(|e| crate::history::ConnectionHistory::format_time_ago(e.last_connected))
        .filter(|s| !s.is_empty())
        .map(|s| s.width())
        .max()
        .unwrap_or(0);
    let cols = Columns::compute(alias_w, host_w, tags_w, tunnel_w, auth_w, has_ping, history_w, content_width);

    // Column header + list body
    let [header_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    render_header(frame, header_area, &cols);

    // Count hosts per group for group headers
    let group_counts: std::collections::HashMap<&str, usize> = {
        let mut counts = std::collections::HashMap::new();
        let mut current_group: Option<&str> = None;
        for item in &app.display_list {
            match item {
                HostListItem::GroupHeader(text) => {
                    current_group = Some(text.as_str());
                }
                HostListItem::Host { .. } => {
                    if let Some(group) = current_group {
                        *counts.entry(group).or_insert(0) += 1;
                    }
                }
            }
        }
        counts
    };

    let mut items: Vec<ListItem> = Vec::new();
    for item in &app.display_list {
        match item {
            HostListItem::GroupHeader(text) => {
                let upper = text.to_uppercase();
                let count = group_counts.get(text.as_str()).copied().unwrap_or(0);
                let label = format!("{} ({}) ", upper, count);
                let fill = cols.content.saturating_sub(label.width());
                let line = Line::from(vec![
                    Span::styled(label, theme::muted()),
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
        }
    }

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol(" ");

    frame.render_stateful_widget(list, list_area, &mut app.ui.list_state);

}

fn render_search_list(frame: &mut Frame, app: &mut App, area: ratatui::layout::Rect) {
    let title = Line::from(vec![
        Span::styled(" purple. ", theme::brand_badge()),
        Span::raw(format!(
            " search: {}/{} ",
            app.search.filtered_indices.len(),
            app.hosts.len()
        )),
    ]);

    let update_title = app.update_available.as_ref().map(|ver| {
        Line::from(Span::styled(
            format!(" v{} available — run '{}' ", ver, app.update_hint),
            theme::update_badge(),
        ))
    });

    if app.search.filtered_indices.is_empty() {
        let mut block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(title)
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

    let content_width = (inner.width as usize).saturating_sub(1);
    let filtered_hosts = || app.search.filtered_indices.iter().filter_map(|&i| app.hosts.get(i));
    let alias_w = filtered_hosts().map(|h| h.alias.width()).max().unwrap_or(8);
    let host_w = filtered_hosts().map(composite_host_width).max().unwrap_or(12);
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
    let cols = Columns::compute(alias_w, host_w, tags_w, tunnel_w, auth_w, has_ping, history_w, content_width);

    let [header_area, list_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(inner);

    render_header(frame, header_area, &cols);

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

    let list = List::new(items)
        .highlight_style(theme::selected_row())
        .highlight_symbol(" ");

    frame.render_stateful_widget(list, list_area, &mut app.ui.list_state);

}

fn render_header(frame: &mut Frame, area: ratatui::layout::Rect, cols: &Columns) {
    let style = theme::bold();
    let gap = " ".repeat(cols.gap);
    let mut spans = vec![
        Span::styled(format!("  {:<width$}", "NAME", width = cols.alias), style),
        Span::raw(gap.clone()),
        Span::styled(format!("{:<width$}", "HOST", width = cols.host), style),
    ];
    if cols.auth > 0 {
        spans.push(Span::raw(gap.clone()));
        spans.push(Span::styled(format!("{:<width$}", "AUTH", width = cols.auth), style));
    }
    if cols.tunnel > 0 {
        spans.push(Span::raw(gap.clone()));
        spans.push(Span::styled(format!("{:<width$}", "TUNNEL", width = cols.tunnel), style));
    }
    if cols.show_ping {
        spans.push(Span::raw(gap.clone()));
        spans.push(Span::styled("PING", style));
    }
    if cols.tags > 0 {
        spans.push(Span::raw(gap.clone()));
        spans.push(Span::styled(format!("{:<width$}", "TAGS", width = cols.tags), style));
    }
    if cols.history > 0 {
        spans.push(Span::raw(gap));
        spans.push(Span::styled(format!("{:>width$}", "LAST", width = cols.history), style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Compute the display width of a host's tags (including provider and source file).
fn host_tags_width(host: &crate::ssh_config::model::HostEntry) -> usize {
    let mut w = 0usize;
    for tag in &host.tags {
        if w > 0 { w += 1; } // space between tags
        w += 1 + tag.width(); // # + tag
    }
    if let Some(ref label) = host.provider {
        if w > 0 { w += 1; }
        w += 1 + label.width();
    }
    if let Some(ref source) = host.source_file {
        let name = source.file_name().map(|f| f.to_string_lossy().width()).unwrap_or(0);
        if name > 0 {
            if w > 0 { w += 1; }
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
    let host_matches = !alias_matches && !q.is_empty()
        && (app::contains_ci(&host.hostname, q)
            || app::contains_ci(&host.user, q));

    let mut spans: Vec<Span> = Vec::new();

    // === NAME column (fixed width) ===
    let alias_style = if alias_matches {
        theme::highlight_bold()
    } else {
        theme::bold()
    };
    let marker = if multi_selected { "\u{2713}" } else { " " };
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
        let user_prefix = if has_user { format!("{}@", host.user) } else { String::new() };
        let port_suffix = if has_port { format!(":{}", host.port) } else { String::new() };

        let available = cols.host;
        let prefix_w = user_prefix.width();
        let suffix_w = port_suffix.width();
        let hostname_budget = available.saturating_sub(prefix_w).saturating_sub(suffix_w);

        if hostname_budget >= 4 || !has_user {
            // Enough room: show user@, truncated hostname, :port
            if has_user {
                spans.push(Span::styled(user_prefix.clone(), theme::muted()));
                host_used += prefix_w;
            }
            let hostname_trunc = super::truncate(&host.hostname, hostname_budget);
            host_used += hostname_trunc.width();
            spans.push(Span::styled(hostname_trunc, Style::default()));
            if has_port {
                spans.push(Span::styled(port_suffix, theme::muted()));
                host_used += suffix_w;
            }
        } else {
            // Very tight: just truncate the whole composite
            host_used = composite_trunc.width();
            spans.push(Span::raw(composite_trunc));
        }
    }

    let host_pad = cols.host.saturating_sub(host_used);
    if host_pad > 0 {
        spans.push(Span::raw(" ".repeat(host_pad)));
    }

    // === AUTH column (SSH key or password source) ===
    if cols.auth > 0 {
        spans.push(Span::raw(gap.clone()));
        let label = auth_label(host);
        if !label.is_empty() {
            spans.push(Span::styled(
                format!("{:<width$}", label, width = cols.auth),
                theme::muted(),
            ));
        } else {
            spans.push(Span::raw(" ".repeat(cols.auth)));
        }
    }

    // === TUNNEL column ===
    if cols.tunnel > 0 {
        spans.push(Span::raw(gap.clone()));
        if let Some(summary) = tunnel_summaries.get(&host.alias) {
            let style = if tunnel_active { theme::success() } else { theme::muted() };
            spans.push(Span::styled(
                format!("{:<width$}", summary, width = cols.tunnel),
                style,
            ));
        } else {
            spans.push(Span::raw(" ".repeat(cols.tunnel)));
        }
    }

    // === PING column ===
    if cols.show_ping {
        spans.push(Span::raw(gap.clone()));
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
    }

    // === TAGS column (fixed width, +N overflow) ===
    if cols.tags > 0 {
        spans.push(Span::raw(gap.clone()));
        let tag_matches = !q.is_empty() && !alias_matches && !host_matches;
        build_tag_column(&mut spans, host, tag_matches, q, cols.tags);
    }

    // === LAST column (right-aligned) ===
    if cols.history > 0 {
        spans.push(Span::raw(gap));
        if let Some(entry) = history.entries.get(&host.alias) {
            let ago = crate::history::ConnectionHistory::format_time_ago(entry.last_connected);
            if !ago.is_empty() {
                spans.push(Span::styled(
                    format!("{:>width$}", ago, width = cols.history),
                    theme::muted(),
                ));
            } else {
                spans.push(Span::raw(" ".repeat(cols.history)));
            }
        } else {
            spans.push(Span::raw(" ".repeat(cols.history)));
        }
    }

    ListItem::new(Line::from(spans))
}

/// Build tag spans that fit within a fixed column width, with +N overflow.
fn build_tag_column(
    spans: &mut Vec<Span<'_>>,
    host: &crate::ssh_config::model::HostEntry,
    tag_matches: bool,
    query: &str,
    width: usize,
) {
    // Collect all tags: user tags + provider + source file
    let mut all_tags: Vec<(String, Style)> = Vec::new();
    for tag in &host.tags {
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

    let mut used = 0usize;
    let mut shown = 0usize;
    for (i, (tag, style)) in all_tags.iter().enumerate() {
        let sep = if shown > 0 { 1 } else { 0 };
        let tag_w = tag.width();
        let remaining = all_tags.len() - i - 1;
        // Reserve space for +N if there are more tags after this one.
        // Use len - i (includes current tag) because overflow count includes it.
        let overflow_count = all_tags.len() - i;
        let overflow_reserve = if remaining > 0 {
            format!(" +{}", overflow_count).width()
        } else {
            0
        };

        if used + sep + tag_w <= width && (remaining == 0 || used + sep + tag_w + overflow_reserve <= width) {
            if shown > 0 { spans.push(Span::raw(" ")); used += 1; }
            spans.push(Span::styled(tag.clone(), *style));
            used += tag_w;
            shown += 1;
        } else {
            // Show +N for all remaining (including this one)
            let count = all_tags.len() - i;
            let overflow = if shown > 0 { format!(" +{}", count) } else { format!("+{}", count) };
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

fn render_search_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let query = app.search.query.as_deref().unwrap_or("");
    let match_info = if query.is_empty() {
        String::new()
    } else {
        let count = app.search.filtered_indices.len();
        match count {
            0 => " (no matches)".to_string(),
            1 => " (1 match)".to_string(),
            n => format!(" ({} matches)", n),
        }
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

fn footer_spans(detail_active: bool, multi_count: usize) -> Vec<Span<'static>> {
    let view_label = if detail_active { " compact " } else { " detail " };
    let mut spans = vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" connect ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("/", theme::accent_bold()),
        Span::styled(" search ", theme::muted()),
        Span::styled("#", theme::accent_bold()),
        Span::styled(" tag ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("a", theme::accent_bold()),
        Span::styled(" add ", theme::muted()),
        Span::styled("e", theme::accent_bold()),
        Span::styled(" edit ", theme::muted()),
        Span::styled("d", theme::accent_bold()),
        Span::styled(" del ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("v", theme::accent_bold()),
        Span::styled(view_label, theme::muted()),
        Span::styled("?", theme::accent_bold()),
        Span::styled(" help", theme::muted()),
    ];
    if multi_count > 0 {
        spans.push(Span::styled("\u{2502} ", theme::muted()));
        spans.push(Span::styled(format!("{} selected", multi_count), theme::accent_bold()));
    }
    spans
}

fn search_footer_spans<'a>() -> Vec<Span<'a>> {
    vec![
        Span::styled(" Enter", theme::primary_action()),
        Span::styled(" connect ", theme::muted()),
        Span::styled("\u{2502} ", theme::muted()),
        Span::styled("Esc", theme::accent_bold()),
        Span::styled(" cancel", theme::muted()),
    ]
}

fn render_tag_bar(frame: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let input = app.tag_input.as_deref().unwrap_or("");
    let tag_line = Line::from(vec![
        Span::styled(" tags: ", theme::accent_bold()),
        Span::raw(input),
        Span::styled("_", theme::accent()),
    ]);
    frame.render_widget(Paragraph::new(tag_line), area);
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
