use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::host_list::format_rtt;

// Box-drawing characters for section cards
const BOX_TL: &str = "\u{256D}"; // ╭
const BOX_TR: &str = "\u{256E}"; // ╮
const BOX_BL: &str = "\u{2570}"; // ╰
const BOX_BR: &str = "\u{256F}"; // ╯
const BOX_H: &str = "\u{2500}"; // ─
const BOX_V: &str = "\u{2502}"; // │

/// Push the opening line of a section card: ╭─ TITLE ───╮
fn section_open(lines: &mut Vec<Line<'static>>, title: &str, width: usize) {
    // prefix: "╭─ " border, then TITLE in bold, then " " — split styling
    let border_prefix = format!("{}\u{2500} ", BOX_TL);
    let title_suffix = " ";
    let prefix_width = border_prefix.width() + title.width() + title_suffix.width();
    let fill = width.saturating_sub(prefix_width).saturating_sub(1); // -1 for TR char
    lines.push(Line::from(vec![
        Span::styled(border_prefix, theme::border()),
        Span::styled(title.to_string(), theme::bold()),
        Span::styled(title_suffix, theme::border()),
        Span::styled(BOX_H.repeat(fill), theme::border()),
        Span::styled(BOX_TR, theme::border()),
    ]));
}

/// Push the opening line of a section card without a title: ╭───────╮
fn section_open_notitle(lines: &mut Vec<Line<'static>>, width: usize) {
    let fill = width.saturating_sub(2); // -1 for TL, -1 for TR
    lines.push(Line::from(vec![
        Span::styled(BOX_TL, theme::border()),
        Span::styled(BOX_H.repeat(fill), theme::border()),
        Span::styled(BOX_TR, theme::border()),
    ]));
}

/// Push a content row wrapped in box side characters: │ <spans...> │
/// Pads content to fill `width` columns (right-aligns the closing │).
fn section_line(lines: &mut Vec<Line<'static>>, spans: Vec<Span<'static>>, width: usize) {
    let mut full_spans: Vec<Span<'static>> =
        vec![Span::styled(format!("{} ", BOX_V), theme::border())];
    let content_width: usize = full_spans.iter().map(|s| s.content.width()).sum::<usize>()
        + spans.iter().map(|s| s.content.width()).sum::<usize>();
    full_spans.extend(spans);
    // Pad to align the right │ border
    let closing_offset = 1; // the │ character
    let padding = width
        .saturating_sub(content_width)
        .saturating_sub(closing_offset);
    if padding > 0 {
        full_spans.push(Span::raw(" ".repeat(padding)));
    }
    full_spans.push(Span::styled(BOX_V, theme::border()));
    lines.push(Line::from(full_spans));
}

/// Push the closing line of a section card: ╰───────╯
fn section_close(lines: &mut Vec<Line<'static>>, width: usize) {
    let fill = width.saturating_sub(2); // -1 for BL, -1 for BR
    lines.push(Line::from(vec![
        Span::styled(BOX_BL, theme::border()),
        Span::styled(BOX_H.repeat(fill), theme::border()),
        Span::styled(BOX_BR, theme::border()),
    ]));
}

/// Push a label+value field row inside a section card.
fn section_field(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    value: &str,
    max_value_width: usize,
    box_width: usize,
) {
    let display = if max_value_width > 0 && value.width() > max_value_width {
        super::truncate(value, max_value_width)
    } else {
        value.to_string()
    };
    let spans = vec![
        Span::styled(
            format!("{:<width$}", label, width = LABEL_WIDTH),
            theme::muted(),
        ),
        Span::styled(display, theme::bold()),
    ];
    section_line(lines, spans, box_width);
}

use super::theme;
use crate::app::App;
use crate::history::ConnectionHistory;
use crate::ssh_config::model::ConfigElement;

const LABEL_WIDTH: usize = 14;

/// Short label for a password source.
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

/// Wrap tags into rows that fit within `max_width` display columns.
/// Each row is a Vec of references into the input slice.
fn wrap_tags<'a>(tags: &'a [String], max_width: usize) -> Vec<Vec<&'a str>> {
    let mut rows: Vec<Vec<&'a str>> = Vec::new();
    let mut current_row: Vec<&'a str> = Vec::new();
    let mut current_width: usize = 0;
    for tag in tags {
        let tag_width = UnicodeWidthStr::width(tag.as_str());
        let needed = if current_width == 0 {
            tag_width
        } else {
            tag_width + 1 // space separator
        };
        if current_width > 0 && current_width + needed > max_width {
            rows.push(std::mem::take(&mut current_row));
            current_width = 0;
        }
        if current_width > 0 {
            current_width += 1; // space
        }
        current_row.push(tag);
        current_width += tag_width;
    }
    if !current_row.is_empty() {
        rows.push(current_row);
    }
    rows
}

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    // Check if a pattern is selected — render pattern detail instead
    if let Some(pattern) = app.selected_pattern() {
        render_pattern_detail(frame, app, area, pattern);
        return;
    }

    let host = match app.selected_host() {
        Some(h) => h,
        None => {
            let empty = Paragraph::new(" Select a host to see details.").style(theme::muted());
            frame.render_widget(empty, area);
            return;
        }
    };

    // box_width = area width; each section card spans the full width.
    // max_value_width = box_width - "│ " prefix (2) - " │" suffix (2) - LABEL_WIDTH
    let box_width = area.width as usize;
    let max_value_width = box_width.saturating_sub(4).saturating_sub(LABEL_WIDTH);

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Header card: alias as title, then user@host:port + status line
    {
        section_open(&mut lines, &host.alias.clone(), box_width);

        let user_display = host.user.as_str();
        let port_display = host.port;
        let host_addr = host.hostname.as_str();
        let addr_str = if !user_display.is_empty() && !host_addr.is_empty() {
            format!("{}@{}:{}", user_display, host_addr, port_display)
        } else if !host_addr.is_empty() {
            format!("{}:{}", host_addr, port_display)
        } else {
            String::new()
        };
        if !addr_str.is_empty() {
            // Available width inside box: box_width - 2 (│ prefix+space) - 1 (closing │)
            let inner = box_width.saturating_sub(3);
            let truncated = super::truncate(&addr_str, inner);
            section_line(
                &mut lines,
                vec![Span::styled(truncated, theme::muted())],
                box_width,
            );
        }

        // Status line using dual-encoded glyphs (consistent with host list)
        let status_spans: Vec<Span<'static>> = match app.ping_status.get(&host.alias) {
            Some(status @ crate::app::PingStatus::Reachable { rtt_ms }) => {
                vec![Span::styled(
                    format!(
                        "{} online ({})",
                        crate::app::status_glyph(Some(status)),
                        format_rtt(*rtt_ms)
                    ),
                    theme::success(),
                )]
            }
            Some(status @ crate::app::PingStatus::Slow { rtt_ms }) => {
                vec![Span::styled(
                    format!(
                        "{} slow ({})",
                        crate::app::status_glyph(Some(status)),
                        format_rtt(*rtt_ms)
                    ),
                    theme::warning(),
                )]
            }
            Some(status @ crate::app::PingStatus::Unreachable) => {
                vec![Span::styled(
                    format!("{} offline", crate::app::status_glyph(Some(status))),
                    theme::error(),
                )]
            }
            Some(status @ crate::app::PingStatus::Checking) => {
                vec![Span::styled(
                    format!("{} checking", crate::app::status_glyph(Some(status))),
                    theme::muted(),
                )]
            }
            Some(crate::app::PingStatus::Skipped) | None => vec![],
        };
        if !status_spans.is_empty() {
            section_line(&mut lines, status_spans, box_width);
        }

        section_close(&mut lines, box_width);
    }

    // Connection section
    section_open(&mut lines, "CONNECTION", box_width);

    section_field(
        &mut lines,
        "Host",
        &host.hostname,
        max_value_width,
        box_width,
    );

    if !host.user.is_empty() {
        section_field(&mut lines, "User", &host.user, max_value_width, box_width);
    }

    if host.port != 22 {
        section_field(
            &mut lines,
            "Port",
            &host.port.to_string(),
            max_value_width,
            box_width,
        );
    }

    if !host.identity_file.is_empty() {
        let key_display = host
            .identity_file
            .rsplit('/')
            .next()
            .unwrap_or(&host.identity_file);
        section_field(&mut lines, "Key", key_display, max_value_width, box_width);
    }

    if let Some(ref askpass) = host.askpass {
        section_field(
            &mut lines,
            "Password",
            password_label(askpass),
            max_value_width,
            box_width,
        );
    }

    if let Some(status) = app.ping_status.get(&host.alias) {
        let ping_text = match status {
            crate::app::PingStatus::Reachable { rtt_ms }
            | crate::app::PingStatus::Slow { rtt_ms } => format_rtt(*rtt_ms),
            crate::app::PingStatus::Unreachable => "--".to_string(),
            crate::app::PingStatus::Skipped => "-- (proxied)".to_string(),
            crate::app::PingStatus::Checking => "...".to_string(),
        };
        section_field(&mut lines, "Ping", &ping_text, max_value_width, box_width);
    }

    if let Some(stale_ts) = host.stale {
        let ago = ConnectionHistory::format_time_ago(stale_ts);
        let stale_value = if ago.is_empty() {
            "yes".to_string()
        } else {
            format!("{} ago", ago)
        };
        let display = if max_value_width > 0 {
            super::truncate(&stale_value, max_value_width)
        } else {
            stale_value
        };
        section_line(
            &mut lines,
            vec![
                Span::styled(
                    format!("{:<width$}", "Stale", width = LABEL_WIDTH),
                    theme::muted(),
                ),
                Span::styled(display, theme::error()),
            ],
            box_width,
        );
    }

    section_close(&mut lines, box_width);

    // Activity section
    let history_entry = app.history.entries.get(&host.alias);

    if history_entry.is_some() {
        // The sparkline chart width is the inner box content width: box_width - 4
        // ("│ " prefix = 2, " │" suffix = 2)
        let chart_width = box_width.saturating_sub(4);
        section_open(&mut lines, "ACTIVITY", box_width);

        if let Some(entry) = history_entry {
            let ago = ConnectionHistory::format_time_ago(entry.last_connected);
            if !ago.is_empty() {
                section_field(
                    &mut lines,
                    "Last SSH",
                    &format!("{} ago", ago),
                    max_value_width,
                    box_width,
                );
            }
            section_field(
                &mut lines,
                "Connections",
                &entry.count.to_string(),
                max_value_width,
                box_width,
            );

            if !entry.timestamps.is_empty() && chart_width >= 10 {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                // Fewer than 3 connections: show a compact text list instead of sparkline
                let recent: Vec<u64> = entry
                    .timestamps
                    .iter()
                    .copied()
                    .filter(|&t| t <= now)
                    .collect();
                if recent.len() < SPARKLINE_MIN_CONNECTIONS {
                    let labels: Vec<String> = recent
                        .iter()
                        .rev()
                        .take(4)
                        .map(|&t| {
                            let ago = ConnectionHistory::format_time_ago(t);
                            if ago.is_empty() {
                                "now".to_string()
                            } else {
                                format!("{} ago", ago)
                            }
                        })
                        .collect();
                    if !labels.is_empty() {
                        let text = labels.join(", ");
                        let truncated = super::truncate(&text, chart_width);
                        section_line(
                            &mut lines,
                            vec![Span::styled(truncated, theme::muted())],
                            box_width,
                        );
                    }
                } else {
                    let chart_lines = activity_sparkline(&entry.timestamps, chart_width);
                    if !chart_lines.is_empty() {
                        // Empty separator row inside the box
                        section_line(&mut lines, vec![], box_width);
                        for chart_line in chart_lines {
                            section_line(
                                &mut lines,
                                chart_line.spans.into_iter().collect(),
                                box_width,
                            );
                        }
                    }
                }
            }
        }

        section_close(&mut lines, box_width);
    }

    // Route visualisation (only when ProxyJump resolves to known hosts)
    if !host.proxy_jump.is_empty() {
        let chain = resolve_proxy_chain(host, &app.hosts);
        if !chain.is_empty() {
            section_open(&mut lines, "ROUTE", box_width);
            // hop_width: content width minus "  ● " prefix (4 chars)
            let hop_width = box_width.saturating_sub(4 + 4); // box borders (4) + indent+bullet (4)
            section_line(
                &mut lines,
                vec![
                    Span::styled("  \u{25CB} ", theme::muted()),
                    Span::styled("you", theme::muted()),
                ],
                box_width,
            );
            for (name, hostname, in_config) in chain.iter().rev() {
                section_line(
                    &mut lines,
                    vec![Span::styled("    \u{2502}", theme::muted())],
                    box_width,
                );
                let name_style = if *in_config {
                    theme::bold()
                } else {
                    theme::error()
                };
                let name_trunc = super::truncate(name, hop_width);
                let remaining = hop_width.saturating_sub(name_trunc.width());
                let ip = if *in_config && name != hostname && remaining > 4 {
                    format!(
                        "  {}",
                        super::truncate(hostname, remaining.saturating_sub(2))
                    )
                } else {
                    String::new()
                };
                section_line(
                    &mut lines,
                    vec![
                        Span::styled("  \u{25CF} ", theme::muted()),
                        Span::styled(name_trunc, name_style),
                        Span::styled(ip, theme::muted()),
                    ],
                    box_width,
                );
            }
            section_line(
                &mut lines,
                vec![Span::styled("    \u{2502}", theme::muted())],
                box_width,
            );
            let alias_trunc = super::truncate(&host.alias, hop_width);
            let remaining = hop_width.saturating_sub(alias_trunc.width());
            let target_ip = if remaining > 4 {
                format!(
                    "  {}",
                    super::truncate(&host.hostname, remaining.saturating_sub(2))
                )
            } else {
                String::new()
            };
            section_line(
                &mut lines,
                vec![
                    Span::styled("  \u{25CF} ", theme::accent()),
                    Span::styled(alias_trunc, theme::bold()),
                    Span::styled(target_ip, theme::muted()),
                ],
                box_width,
            );
            section_close(&mut lines, box_width);
        }
    }

    // Tags section
    if !host.tags.is_empty() || !host.provider_tags.is_empty() || host.provider.is_some() {
        section_open(&mut lines, "TAGS", box_width);

        let mut all_tags: Vec<String> = host
            .provider_tags
            .iter()
            .chain(host.tags.iter())
            .cloned()
            .collect();
        if let Some(ref provider) = host.provider {
            all_tags.push(provider.clone());
        }
        // Tag rows fit within box content width: box_width - 4 ("│ " + " │")
        let tag_content_width = box_width.saturating_sub(4);
        for row in wrap_tags(&all_tags, tag_content_width) {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, tag) in row.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(tag.to_string(), theme::accent()));
            }
            section_line(&mut lines, spans, box_width);
        }

        section_close(&mut lines, box_width);
    }

    // Provider metadata section
    if !host.provider_meta.is_empty() {
        let header = match host.provider.as_deref() {
            Some(name) => crate::providers::provider_display_name(name).to_uppercase(),
            None => "PROVIDER".to_string(),
        };
        section_open(&mut lines, &header, box_width);

        for (key, value) in &host.provider_meta {
            let label = meta_label(key);
            section_field(&mut lines, &label, value, max_value_width, box_width);
        }

        section_close(&mut lines, box_width);
    }

    // Tunnels section
    let tunnel_active = app.active_tunnels.contains_key(&host.alias);
    if host.tunnel_count > 0 {
        let tunnel_label = if tunnel_active {
            "TUNNELS (active)"
        } else {
            "TUNNELS"
        };
        section_open(&mut lines, tunnel_label, box_width);

        let rules = find_tunnel_rules(&app.config.elements, &host.alias);
        let style = if tunnel_active {
            theme::bold()
        } else {
            theme::muted()
        };
        let rule_content_width = box_width.saturating_sub(4);
        for rule in &rules {
            let truncated = super::truncate(rule, rule_content_width);
            section_line(&mut lines, vec![Span::styled(truncated, style)], box_width);
        }

        section_close(&mut lines, box_width);
    }

    // Snippets hint
    let snippet_count = app.snippet_store.snippets.len();
    if snippet_count > 0 {
        section_open(&mut lines, "SNIPPETS", box_width);
        let msg = format!("{} available (r to run)", snippet_count);
        section_line(
            &mut lines,
            vec![Span::styled(msg, theme::muted())],
            box_width,
        );
        section_close(&mut lines, box_width);
    }

    // Containers section (only shown when cache data exists)
    if let Some(cache_entry) = app.container_cache.get(&host.alias) {
        section_open(&mut lines, "CONTAINERS", box_width);
        let running = cache_entry
            .containers
            .iter()
            .filter(|c| c.state == "running")
            .count();
        let total = cache_entry.containers.len();
        section_field(
            &mut lines,
            "Total",
            &format!("{} running / {} total", running, total),
            max_value_width,
            box_width,
        );
        section_field(
            &mut lines,
            "Runtime",
            cache_entry.runtime.as_str(),
            max_value_width,
            box_width,
        );
        section_field(
            &mut lines,
            "Last checked",
            &crate::containers::format_relative_time(cache_entry.timestamp),
            max_value_width,
            box_width,
        );
        for container in &cache_entry.containers {
            let (icon, icon_style) = match container.state.as_str() {
                "running" => ("\u{2713}", theme::success()),
                "exited" | "dead" => ("\u{2717}", theme::error()),
                _ => ("\u{25cf}", theme::bold()),
            };
            let name = crate::containers::truncate_str(
                &container.names,
                max_value_width.saturating_sub(2),
            );
            section_line(
                &mut lines,
                vec![
                    Span::styled(
                        format!("{:>width$}", "", width = LABEL_WIDTH),
                        theme::muted(),
                    ),
                    Span::styled(icon, icon_style),
                    Span::styled(" ", theme::muted()),
                    Span::styled(name, theme::bold()),
                ],
                box_width,
            );
        }
        section_close(&mut lines, box_width);
    }

    // Inherited directives section — match against alias and hostname for display.
    // OpenSSH Host keyword matches alias only, but patterns like "10.30.0.*" apply
    // when the user types the IP directly, so we show those too.
    let mut inherited = app.config.matching_patterns(&host.alias);
    if !host.hostname.is_empty() {
        let hostname_matches = app.config.matching_patterns(&host.hostname);
        for entry in hostname_matches {
            if !inherited.iter().any(|e| e.pattern == entry.pattern) {
                inherited.push(entry);
            }
        }
    }
    for pattern_entry in &inherited {
        section_open(&mut lines, "PATTERN MATCH", box_width);
        section_line(
            &mut lines,
            vec![Span::styled(
                super::truncate(&pattern_entry.pattern, box_width.saturating_sub(4)),
                theme::bold(),
            )],
            box_width,
        );
        for (key, value) in &pattern_entry.directives {
            section_field(&mut lines, key, value, max_value_width, box_width);
        }
        section_close(&mut lines, box_width);
    }

    // Source section (for included hosts)
    if let Some(ref source) = host.source_file {
        section_open_notitle(&mut lines, box_width);
        section_field(
            &mut lines,
            "Source",
            &source.display().to_string(),
            max_value_width,
            box_width,
        );
        section_close(&mut lines, box_width);
    }

    // Stretch: give all remaining vertical space to the last section card.
    // Insert empty bordered lines before the last section_close line.
    let available = area.height as usize;
    if lines.len() < available {
        let extra = available - lines.len();
        // Find the last section_close line (╰...╯)
        if let Some(last_close) = lines.iter().rposition(|line| {
            line.spans
                .first()
                .map(|s| s.content.starts_with(BOX_BL))
                .unwrap_or(false)
        }) {
            for _ in 0..extra {
                lines.insert(last_close, section_empty_line(box_width));
            }
        }
    }

    let paragraph = Paragraph::new(lines).scroll((app.ui.detail_scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Empty bordered line for padding: │                              │
fn section_empty_line(width: usize) -> Line<'static> {
    let fill = width.saturating_sub(2);
    Line::from(vec![
        Span::styled(BOX_V, theme::border()),
        Span::raw(" ".repeat(fill)),
        Span::styled(BOX_V, theme::border()),
    ])
}

fn render_pattern_detail(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    pattern: &crate::ssh_config::model::PatternEntry,
) {
    let box_width = area.width as usize;
    let max_value_width = box_width.saturating_sub(4).saturating_sub(LABEL_WIDTH);

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Header card: PATTERN MATCH with pattern on first line
    section_open(&mut lines, "PATTERN MATCH", box_width);
    section_line(
        &mut lines,
        vec![Span::styled(pattern.pattern.clone(), theme::bold())],
        box_width,
    );
    section_close(&mut lines, box_width);

    // Directives section
    if !pattern.directives.is_empty() {
        section_open(&mut lines, "DIRECTIVES", box_width);
        for (key, value) in &pattern.directives {
            section_field(&mut lines, key, value, max_value_width, box_width);
        }
        section_close(&mut lines, box_width);
    }

    // Tags section
    if !pattern.tags.is_empty() {
        section_open(&mut lines, "TAGS", box_width);
        let tag_strings: Vec<String> = pattern.tags.to_vec();
        let inner_width = box_width.saturating_sub(4);
        let tag_rows = wrap_tags(&tag_strings, inner_width);
        for row in &tag_rows {
            section_line(
                &mut lines,
                vec![Span::styled(row.join(" "), theme::accent())],
                box_width,
            );
        }
        section_close(&mut lines, box_width);
    }

    // Matches section
    let matching_aliases: Vec<String> = app
        .hosts
        .iter()
        .filter(|h| {
            crate::ssh_config::model::host_pattern_matches(&pattern.pattern, &h.alias)
                || (!h.hostname.is_empty()
                    && crate::ssh_config::model::host_pattern_matches(
                        &pattern.pattern,
                        &h.hostname,
                    ))
        })
        .map(|h| h.alias.clone())
        .collect();

    if !matching_aliases.is_empty() {
        section_open(
            &mut lines,
            &format!("MATCHES ({})", matching_aliases.len()),
            box_width,
        );
        let inner_width = box_width.saturating_sub(4);
        for alias in &matching_aliases {
            section_line(
                &mut lines,
                vec![Span::styled(
                    super::truncate(alias, inner_width),
                    theme::bold(),
                )],
                box_width,
            );
        }
        section_close(&mut lines, box_width);
    }

    // Source file
    if let Some(ref source) = pattern.source_file {
        section_open(&mut lines, "SOURCE", box_width);
        section_field(
            &mut lines,
            "File",
            &source.display().to_string(),
            max_value_width,
            box_width,
        );
        section_close(&mut lines, box_width);
    }

    // Stretch: give all remaining vertical space to the last section card.
    let available = area.height as usize;
    if lines.len() < available {
        let extra = available - lines.len();
        if let Some(last_close) = lines.iter().rposition(|line| {
            line.spans
                .first()
                .map(|s| s.content.starts_with(BOX_BL))
                .unwrap_or(false)
        }) {
            for _ in 0..extra {
                lines.insert(last_close, section_empty_line(box_width));
            }
        }
    }

    let paragraph = Paragraph::new(lines).scroll((app.ui.detail_scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Resolve the ProxyJump chain for a host. Returns the list of hops from
/// the user's machine to the target: [(alias_or_name, hostname, in_config)].
/// Follows ProxyJump directives through the config (max 10 hops to prevent loops).
fn resolve_proxy_chain(
    host: &crate::ssh_config::model::HostEntry,
    hosts: &[crate::ssh_config::model::HostEntry],
) -> Vec<(String, String, bool)> {
    let mut chain = Vec::new();
    let mut current_jump = host.proxy_jump.clone();
    let mut seen = std::collections::HashSet::new();
    seen.insert(host.alias.clone()); // Prevent loops back to the target host
    for _ in 0..10 {
        if current_jump.is_empty() || current_jump.eq_ignore_ascii_case("none") {
            break;
        }
        // ProxyJump can be comma-separated for multi-hop: host1,host2
        // SSH processes them left to right (first hop first)
        let hops: Vec<&str> = current_jump.split(',').map(|s| s.trim()).collect();
        for hop_name in &hops {
            if hop_name.is_empty() {
                continue;
            }
            let name = hop_name.to_string();
            if !seen.insert(name.clone()) {
                // Loop detected
                return chain;
            }
            if let Some(jump_host) = hosts.iter().find(|h| h.alias == name) {
                chain.push((name, jump_host.hostname.clone(), true));
            } else {
                // Host not in config (external or typo)
                chain.push((name.clone(), name, false));
            }
        }
        // Follow the chain: check the last hop's ProxyJump
        let last_hop = hops.last().unwrap_or(&"");
        if let Some(next) = hosts.iter().find(|h| h.alias == *last_hop) {
            current_jump = next.proxy_jump.clone();
        } else {
            break;
        }
    }
    chain
}

/// Minimum number of connections before showing a sparkline chart.
/// Below this threshold, a compact text list is shown instead.
const SPARKLINE_MIN_CONNECTIONS: usize = 3;

/// Map metadata keys to human-readable labels.
fn meta_label(key: &str) -> String {
    match key {
        "region" => "Region".to_string(),
        "zone" => "Zone".to_string(),
        "datacenter" => "Datacenter".to_string(), // legacy, pre-2.6.0
        "location" => "Location".to_string(),
        "instance" => "Instance".to_string(),
        "size" => "Size".to_string(),
        "machine" => "Machine".to_string(),
        "vm_size" => "VM Size".to_string(),
        "plan" => "Plan".to_string(),
        "specs" => "Specs".to_string(),
        "type" => "Type".to_string(),
        "shape" => "Shape".to_string(),
        "os" => "OS".to_string(),
        "image" => "Image".to_string(),
        "status" => "State".to_string(),
        "node" => "Node".to_string(),
        other => {
            // Capitalize first letter
            let mut chars = other.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

// Block sparkline using lower block elements (▁▂▃▄▅▆▇█).
// 2 rows tall = 16 height levels. Auto-scales from 5 days to 1 year.
// History retains 365 days of timestamps; chart range adapts to data age.
const BLOCKS: [char; 9] = [
    ' ', '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
    '\u{2588}',
];
/// Predefined time ranges for auto-scaling the sparkline.
/// The smallest range that contains the oldest timestamp is used.
/// Predefined time ranges for auto-scaling the sparkline.
/// (days, left_label, midpoint_label)
const CHART_RANGES: &[(u64, &str, &str)] = &[
    (5, "5d", "~2d"),
    (10, "10d", "~5d"),
    (14, "2w", "~1w"),
    (21, "3w", "~10d"),
    (30, "30d", "~2w"),
    (60, "2mo", "~1mo"),
    (84, "12w", "~6w"),
    (180, "6mo", "~3mo"),
    (365, "1y", "~6mo"),
];

fn activity_sparkline(timestamps: &[u64], chart_width: usize) -> Vec<Line<'static>> {
    if chart_width == 0 {
        return Vec::new();
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Auto-scale: pick the smallest range that contains the oldest timestamp
    let oldest = timestamps
        .iter()
        .copied()
        .filter(|&t| t <= now)
        .min()
        .unwrap_or(now);
    let data_age_days = now.saturating_sub(oldest) / 86400 + 1;
    let chart_days = CHART_RANGES
        .iter()
        .find(|(days, _, _)| *days >= data_age_days)
        .map(|(days, _, _)| *days)
        .unwrap_or(CHART_RANGES.last().unwrap().0);

    let range_secs = chart_days * 86400;
    let bucket_secs = range_secs as f64 / chart_width as f64;
    let cutoff = now.saturating_sub(range_secs);

    let mut buckets = vec![0u64; chart_width];
    for &ts in timestamps {
        if ts < cutoff || ts > now {
            continue;
        }
        let age = now.saturating_sub(ts);
        let idx =
            chart_width - 1 - ((age as f64 / bucket_secs).floor() as usize).min(chart_width - 1);
        buckets[idx] += 1;
    }

    if buckets.iter().all(|&v| v == 0) {
        return Vec::new();
    }

    let max_val = buckets.iter().copied().max().unwrap_or(1).max(1);
    let total_levels = 16usize; // 2 rows x 8 levels

    let heights: Vec<usize> = buckets
        .iter()
        .map(|&v| {
            if v == 0 {
                0
            } else {
                ((v as f64 / max_val as f64) * total_levels as f64).ceil() as usize
            }
        })
        .collect();

    let mut chart_lines = Vec::new();

    // Top row (only rendered if any bar exceeds half height)
    if heights.iter().any(|&h| h > 8) {
        let mut top = String::with_capacity(chart_width * 3);
        for &h in &heights {
            if h > 8 {
                top.push(BLOCKS[(h - 8).min(8)]);
            } else {
                top.push(' ');
            }
        }
        chart_lines.push(Line::from(Span::styled(top, theme::bold())));
    }

    // Bottom row with dotted baseline for empty buckets
    let mut bottom_spans: Vec<Span<'static>> = Vec::new();
    let mut run_empty = String::new();
    let mut run_filled = String::new();

    for &h in &heights {
        if h == 0 {
            if !run_filled.is_empty() {
                bottom_spans.push(Span::styled(std::mem::take(&mut run_filled), theme::bold()));
            }
            run_empty.push('\u{00B7}'); // · (middle dot)
        } else {
            if !run_empty.is_empty() {
                bottom_spans.push(Span::styled(std::mem::take(&mut run_empty), theme::muted()));
            }
            if h >= 8 {
                run_filled.push(BLOCKS[8]);
            } else {
                run_filled.push(BLOCKS[h]);
            }
        }
    }
    // Flush remaining runs
    if !run_filled.is_empty() {
        bottom_spans.push(Span::styled(run_filled, theme::bold()));
    }
    if !run_empty.is_empty() {
        bottom_spans.push(Span::styled(run_empty, theme::muted()));
    }
    chart_lines.push(Line::from(bottom_spans));

    // Axis labels: left ... midpoint ... now
    let range_entry = CHART_RANGES.iter().find(|(days, _, _)| *days == chart_days);
    let left_label = range_entry
        .map(|(_, label, _)| label.to_string())
        .unwrap_or_else(|| format!("{}d", chart_days));
    let mid_label = range_entry
        .map(|(_, _, mid)| mid.to_string())
        .unwrap_or_default();
    let right_label = "now";

    let labels_width = left_label.len() + mid_label.len() + right_label.len();
    if !mid_label.is_empty() && chart_width > labels_width + 4 {
        // Three-point axis: left ... mid ... now
        let total_gap = chart_width.saturating_sub(labels_width);
        let gap_left = total_gap / 2;
        let gap_right = total_gap - gap_left;
        chart_lines.push(Line::from(vec![
            Span::styled(left_label, theme::muted()),
            Span::raw(" ".repeat(gap_left)),
            Span::styled(mid_label, theme::muted()),
            Span::raw(" ".repeat(gap_right)),
            Span::styled(right_label.to_string(), theme::muted()),
        ]));
    } else {
        // Two-point axis (narrow panel): left ... now
        let gap = chart_width.saturating_sub(left_label.len() + right_label.len());
        chart_lines.push(Line::from(vec![
            Span::styled(left_label, theme::muted()),
            Span::raw(" ".repeat(gap)),
            Span::styled(right_label.to_string(), theme::muted()),
        ]));
    }

    chart_lines
}

fn find_tunnel_rules(elements: &[ConfigElement], alias: &str) -> Vec<String> {
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
                        Some(format!("{} {}", prefix, d.value))
                    })
                    .collect();
            }
            ConfigElement::Include(include) => {
                for file in &include.resolved_files {
                    let result = find_tunnel_rules(&file.elements, alias);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn sparkline_empty_timestamps() {
        let result = activity_sparkline(&[], 40);
        assert!(result.is_empty());
    }

    #[test]
    fn sparkline_all_outside_range() {
        let old = now() - 400 * 86400; // older than max range (365d)
        let result = activity_sparkline(&[old], 40);
        assert!(result.is_empty());
    }

    #[test]
    fn sparkline_single_timestamp() {
        let ts = now() - 86400;
        let lines = activity_sparkline(&[ts], 40);
        assert!(!lines.is_empty());
        // Bottom row + axis = at least 2 lines
        assert!(lines.len() >= 2);
    }

    #[test]
    fn sparkline_multiple_buckets() {
        let n = now();
        let timestamps: Vec<u64> = (0..84).map(|day| n - day * 86400).collect();
        let lines = activity_sparkline(&timestamps, 40);
        assert!(lines.len() >= 2);
    }

    #[test]
    fn sparkline_all_in_one_bucket() {
        let n = now();
        let timestamps: Vec<u64> = (0..10).map(|i| n - i * 60).collect();
        let lines = activity_sparkline(&timestamps, 20);
        assert!(lines.len() >= 2);
    }

    #[test]
    fn sparkline_axis_labels() {
        let ts = now() - 86400; // 1 day ago → auto-scales to 5d range
        let lines = activity_sparkline(&[ts], 30);
        let axis = lines.last().unwrap();
        let text: String = axis.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("5d"));
        assert!(text.contains("now"));
    }

    #[test]
    fn sparkline_auto_scales_to_data_range() {
        // 3 days of data → 5d range
        let lines_3d = activity_sparkline(&[now() - 3 * 86400], 30);
        let axis_3d: String = lines_3d
            .last()
            .unwrap()
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(axis_3d.contains("5d"));

        // 8 days of data → 10d range
        let lines_8d = activity_sparkline(&[now() - 8 * 86400], 30);
        let axis_8d: String = lines_8d
            .last()
            .unwrap()
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(axis_8d.contains("10d"));

        // 50 days of data → 2mo range
        let lines_50d = activity_sparkline(&[now() - 50 * 86400], 30);
        let axis_50d: String = lines_50d
            .last()
            .unwrap()
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(axis_50d.contains("2mo"));

        // 100 days of data → 6mo range
        let lines_100d = activity_sparkline(&[now() - 100 * 86400], 30);
        let axis_100d: String = lines_100d
            .last()
            .unwrap()
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(axis_100d.contains("6mo"));
    }

    #[test]
    fn sparkline_shown_at_threshold() {
        // 3 connections (= SPARKLINE_MIN_CONNECTIONS) → sparkline should render
        let n = now();
        let ts = vec![n - 86400, n - 2 * 86400, n - 3 * 86400];
        let lines = activity_sparkline(&ts, 30);
        assert!(
            !lines.is_empty(),
            "sparkline must render at {} connections",
            SPARKLINE_MIN_CONNECTIONS
        );
    }

    #[test]
    fn sparkline_shown_above_threshold() {
        // 4 connections (above threshold) → sparkline should render
        let n = now();
        let ts = vec![n - 3600, n - 86400, n - 2 * 86400, n - 3 * 86400];
        let lines = activity_sparkline(&ts, 30);
        assert!(!lines.is_empty(), "sparkline must render at 4 connections");
    }

    #[test]
    fn sparkline_rendered_with_dotted_baseline() {
        // Verify that empty buckets use · (middle dot) not spaces
        let n = now();
        // One connection at start of range → most buckets empty → dots visible
        let lines = activity_sparkline(&[n - 4 * 86400], 20);
        assert!(!lines.is_empty());
        // Bottom row (before axis) should contain · for empty buckets
        let bottom = &lines[lines.len() - 2]; // second to last = bottom row
        let text: String = bottom.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains('\u{00B7}'),
            "empty buckets should show · (middle dot), got: {:?}",
            text
        );
    }

    #[test]
    fn sparkline_midpoint_label_shown_at_normal_width() {
        // At 30 cols, midpoint label should appear
        let lines = activity_sparkline(&[now() - 86400], 30);
        let axis: String = lines
            .last()
            .unwrap()
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            axis.contains("~2d"),
            "midpoint label missing at 30 cols, got: {:?}",
            axis
        );
    }

    #[test]
    fn sparkline_midpoint_label_hidden_at_narrow_width() {
        // At 10 cols, midpoint label should NOT appear (too narrow)
        let lines = activity_sparkline(&[now() - 86400], 10);
        let axis: String = lines
            .last()
            .unwrap()
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            !axis.contains("~"),
            "midpoint label should be hidden at 10 cols, got: {:?}",
            axis
        );
    }

    #[test]
    fn sparkline_365_day_boundary_selects_1y() {
        // Timestamp at exactly 364 days old → 1y range
        let lines_364 = activity_sparkline(&[now() - 364 * 86400], 30);
        assert!(!lines_364.is_empty(), "364-day-old data should render");
        let axis: String = lines_364
            .last()
            .unwrap()
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            axis.contains("1y"),
            "364 days should use 1y range, got: {axis:?}"
        );
    }

    #[test]
    fn sparkline_narrow_width() {
        let ts = now() - 86400;
        let lines = activity_sparkline(&[ts], 10);
        assert!(lines.len() >= 2);
    }

    #[test]
    fn sparkline_two_rows_for_high_variance() {
        let n = now();
        // One bucket with many hits, rest with few
        let mut timestamps: Vec<u64> = vec![n; 100];
        timestamps.push(n - 40 * 86400);
        let lines = activity_sparkline(&timestamps, 20);
        // Should have top row + bottom row + axis = 3 lines
        assert_eq!(lines.len(), 3);
    }

    // =========================================================================
    // wrap_tags
    // =========================================================================

    fn tags(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn wrap_tags_single_row() {
        let t = tags(&["prod", "web"]);
        let rows = wrap_tags(&t, 32);
        assert_eq!(rows, vec![vec!["prod", "web"]]);
    }

    #[test]
    fn wrap_tags_wraps_to_second_row() {
        let t = tags(&["production", "web", "europe", "api"]);
        // "production web" = 14 cols, "europe" would make 21 > 20
        let rows = wrap_tags(&t, 20);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["production", "web"]);
        assert_eq!(rows[1], vec!["europe", "api"]);
    }

    #[test]
    fn wrap_tags_one_per_row_when_narrow() {
        let t = tags(&["production", "staging"]);
        // Each tag is 10 chars, panel only 10 wide — no room for two
        let rows = wrap_tags(&t, 10);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["production"]);
        assert_eq!(rows[1], vec!["staging"]);
    }

    #[test]
    fn wrap_tags_empty() {
        let rows = wrap_tags(&[], 32);
        assert!(rows.is_empty());
    }

    #[test]
    fn wrap_tags_exact_fit() {
        let t = tags(&["ab", "cd"]);
        // "ab cd" = 5 cols
        let rows = wrap_tags(&t, 5);
        assert_eq!(rows, vec![vec!["ab", "cd"]]);
    }

    #[test]
    fn wrap_tags_exact_overflow() {
        let t = tags(&["ab", "cd"]);
        // "ab cd" = 5 cols, max 4 → wraps
        let rows = wrap_tags(&t, 4);
        assert_eq!(rows.len(), 2);
    }

    // --- resolve_proxy_chain tests ---

    fn host(alias: &str, hostname: &str, proxy: &str) -> crate::ssh_config::model::HostEntry {
        crate::ssh_config::model::HostEntry {
            alias: alias.to_string(),
            hostname: hostname.to_string(),
            proxy_jump: proxy.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn proxy_chain_single_hop() {
        let target = host("server", "10.0.0.1", "bastion");
        let bastion = host("bastion", "1.2.3.4", "");
        let hosts = vec![target.clone(), bastion];
        let chain = resolve_proxy_chain(&target, &hosts);
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].0, "bastion");
        assert_eq!(chain[0].1, "1.2.3.4");
        assert!(chain[0].2); // in_config
    }

    #[test]
    fn proxy_chain_multi_hop() {
        let target = host("server", "10.0.0.1", "jump1");
        let jump1 = host("jump1", "1.1.1.1", "jump2");
        let jump2 = host("jump2", "2.2.2.2", "");
        let hosts = vec![target.clone(), jump1, jump2];
        let chain = resolve_proxy_chain(&target, &hosts);
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].0, "jump1");
        assert_eq!(chain[1].0, "jump2");
    }

    #[test]
    fn proxy_chain_loop_detection() {
        let a = host("a", "1.1.1.1", "b");
        let b = host("b", "2.2.2.2", "a");
        let hosts = vec![a.clone(), b];
        let chain = resolve_proxy_chain(&a, &hosts);
        // Should stop after "b" because "a" was already seen
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].0, "b");
    }

    #[test]
    fn proxy_chain_comma_separated() {
        let target = host("server", "10.0.0.1", "hop1, hop2");
        let hop1 = host("hop1", "1.1.1.1", "");
        let hop2 = host("hop2", "2.2.2.2", "");
        let hosts = vec![target.clone(), hop1, hop2];
        let chain = resolve_proxy_chain(&target, &hosts);
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].0, "hop1");
        assert_eq!(chain[1].0, "hop2");
    }

    #[test]
    fn proxy_chain_host_not_in_config() {
        let target = host("server", "10.0.0.1", "unknown");
        let hosts = vec![target.clone()];
        let chain = resolve_proxy_chain(&target, &hosts);
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].0, "unknown");
        assert_eq!(chain[0].1, "unknown"); // hostname == alias for unknown hosts
        assert!(!chain[0].2); // NOT in_config
    }

    #[test]
    fn proxy_chain_empty_hops_in_comma_list() {
        let target = host("server", "10.0.0.1", "hop1,,hop2");
        let hop1 = host("hop1", "1.1.1.1", "");
        let hop2 = host("hop2", "2.2.2.2", "");
        let hosts = vec![target.clone(), hop1, hop2];
        let chain = resolve_proxy_chain(&target, &hosts);
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].0, "hop1");
        assert_eq!(chain[1].0, "hop2");
    }

    #[test]
    fn proxy_chain_mixed_known_unknown() {
        let target = host("server", "10.0.0.1", "known, mystery, also_known");
        let known = host("known", "1.1.1.1", "");
        let also_known = host("also_known", "3.3.3.3", "");
        let hosts = vec![target.clone(), known, also_known];
        let chain = resolve_proxy_chain(&target, &hosts);
        assert_eq!(chain.len(), 3);
        assert!(chain[0].2); // known: in_config
        assert!(!chain[1].2); // mystery: NOT in_config
        assert!(chain[2].2); // also_known: in_config
    }

    #[test]
    fn proxy_chain_none_stops() {
        let target = host("server", "10.0.0.1", "none");
        let hosts = vec![target.clone()];
        let chain = resolve_proxy_chain(&target, &hosts);
        assert!(chain.is_empty());
    }

    #[test]
    fn proxy_chain_empty_proxyjump() {
        let target = host("server", "10.0.0.1", "");
        let hosts = vec![target.clone()];
        let chain = resolve_proxy_chain(&target, &hosts);
        assert!(chain.is_empty());
    }

    #[test]
    fn proxy_chain_max_depth() {
        // Create a chain of 12 hops (exceeds max 10)
        let mut hosts = Vec::new();
        for i in 0..12 {
            let proxy = if i < 11 {
                format!("h{}", i + 1)
            } else {
                String::new()
            };
            hosts.push(host(&format!("h{}", i), &format!("10.0.0.{}", i), &proxy));
        }
        let target = host("target", "10.0.0.99", "h0");
        hosts.push(target.clone());
        let chain = resolve_proxy_chain(&target, &hosts);
        assert!(chain.len() <= 10);
    }

    // =========================================================================
    // password_label tests
    // =========================================================================

    #[test]
    fn password_label_keychain() {
        assert_eq!(password_label("keychain"), "keychain");
    }

    #[test]
    fn password_label_1password() {
        assert_eq!(password_label("op://vault/item"), "1password");
    }

    #[test]
    fn password_label_bitwarden() {
        assert_eq!(password_label("bw:some-id"), "bitwarden");
    }

    #[test]
    fn password_label_pass() {
        assert_eq!(password_label("pass:entry"), "pass");
    }

    #[test]
    fn password_label_vault() {
        assert_eq!(password_label("vault:secret/path"), "vault");
    }

    #[test]
    fn password_label_custom() {
        assert_eq!(password_label("/usr/bin/my-askpass"), "custom");
    }
}
