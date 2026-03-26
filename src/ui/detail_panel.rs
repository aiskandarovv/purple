use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Padding, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, PingStatus};
use crate::history::ConnectionHistory;
use crate::ssh_config::model::ConfigElement;

const LABEL_WIDTH: usize = 14;

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
    let host = match app.selected_host() {
        Some(h) => h,
        None => {
            let block = Block::bordered()
                .border_type(BorderType::Rounded)
                .padding(Padding::horizontal(1))
                .border_style(theme::border());
            let empty = Paragraph::new(" Select a host to see details.")
                .style(theme::muted())
                .block(block);
            frame.render_widget(empty, area);
            return;
        }
    };

    let title = format!(" {} ", host.alias);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .padding(Padding::horizontal(1))
        .title(Span::styled(title, theme::brand()))
        .border_style(theme::border());

    let inner_width = (area.width as usize).saturating_sub(4); // minus borders + padding
    let max_value_width = inner_width.saturating_sub(LABEL_WIDTH); // minus label

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Connection section
    lines.push(Line::from(""));
    lines.push(section_header("Connection"));

    push_field(&mut lines, "Host", &host.hostname, max_value_width);

    if !host.user.is_empty() {
        push_field(&mut lines, "User", &host.user, max_value_width);
    }

    if host.port != 22 {
        push_field(&mut lines, "Port", &host.port.to_string(), max_value_width);
    }

    if !host.identity_file.is_empty() {
        let key_display = host
            .identity_file
            .rsplit('/')
            .next()
            .unwrap_or(&host.identity_file);
        push_field(&mut lines, "Key", key_display, max_value_width);
    }

    if let Some(ref askpass) = host.askpass {
        push_field(&mut lines, "Password", askpass, max_value_width);
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
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<width$}", "Stale", width = LABEL_WIDTH),
                theme::muted(),
            ),
            Span::styled(display, theme::error()),
        ]));
    }

    // Activity section
    let history_entry = app.history.entries.get(&host.alias);
    let ping = app.ping_status.get(&host.alias);

    if history_entry.is_some() || ping.is_some() {
        lines.push(Line::from(""));
        lines.push(section_header("Activity"));

        if let Some(entry) = history_entry {
            let ago = ConnectionHistory::format_time_ago(entry.last_connected);
            if !ago.is_empty() {
                push_field(
                    &mut lines,
                    "Last SSH",
                    &format!("{} ago", ago),
                    max_value_width,
                );
            }
            push_field(
                &mut lines,
                "Connections",
                &entry.count.to_string(),
                max_value_width,
            );

            if !entry.timestamps.is_empty() && inner_width >= 10 {
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
                        lines.push(Line::from(Span::styled(
                            super::truncate(&text, inner_width),
                            theme::muted(),
                        )));
                    }
                } else {
                    let chart_lines = activity_sparkline(&entry.timestamps, inner_width);
                    if !chart_lines.is_empty() {
                        lines.push(Line::from(""));
                        lines.extend(chart_lines);
                    }
                }
            }
        }

        if let Some(status) = ping {
            let (text, style) = match status {
                PingStatus::Checking => ("checking...", theme::muted()),
                PingStatus::Reachable => ("reachable", theme::success()),
                PingStatus::Unreachable => ("unreachable", theme::error()),
                PingStatus::Skipped => ("skipped", theme::muted()),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:<width$}", "Status", width = LABEL_WIDTH),
                    theme::muted(),
                ),
                Span::styled(text, style),
            ]));
        }
    }

    // Route visualisation (only when ProxyJump resolves to known hosts)
    if !host.proxy_jump.is_empty() {
        let chain = resolve_proxy_chain(host, &app.hosts);
        if !chain.is_empty() {
            lines.push(Line::from(""));
            lines.push(section_header("Route"));
            let indent = "  ";
            let hop_width = inner_width.saturating_sub(4); // minus "  ● "
            lines.push(Line::from(vec![
                Span::styled(format!("{}\u{25CB} ", indent), theme::muted()),
                Span::styled("you", theme::muted()),
            ]));
            for (name, hostname, in_config) in chain.iter().rev() {
                lines.push(Line::from(Span::styled(
                    format!("{}  \u{2502}", indent),
                    theme::muted(),
                )));
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
                lines.push(Line::from(vec![
                    Span::styled(format!("{}\u{25CF} ", indent), theme::muted()),
                    Span::styled(name_trunc, name_style),
                    Span::styled(ip, theme::muted()),
                ]));
            }
            lines.push(Line::from(Span::styled(
                format!("{}  \u{2502}", indent),
                theme::muted(),
            )));
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
            lines.push(Line::from(vec![
                Span::styled(format!("{}\u{25CF} ", indent), theme::accent()),
                Span::styled(alias_trunc, theme::bold()),
                Span::styled(target_ip, theme::muted()),
            ]));
        }
    }

    // Tags section
    if !host.tags.is_empty() || !host.provider_tags.is_empty() || host.provider.is_some() {
        lines.push(Line::from(""));
        lines.push(section_header("Tags"));

        let mut all_tags: Vec<String> = host
            .provider_tags
            .iter()
            .chain(host.tags.iter())
            .map(|t| format!("#{}", t))
            .collect();
        if let Some(ref provider) = host.provider {
            all_tags.push(format!("#{}", provider));
        }
        // Inner width = area width - 2 (borders) - 2 (1-char padding each side).
        let max_width = (area.width as usize).saturating_sub(4);
        for row in wrap_tags(&all_tags, max_width) {
            let mut spans = Vec::new();
            for (i, tag) in row.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.push(Span::styled(tag.to_string(), theme::accent()));
            }
            lines.push(Line::from(spans));
        }
    }

    // Provider metadata section
    if !host.provider_meta.is_empty() {
        lines.push(Line::from(""));
        let header = match host.provider.as_deref() {
            Some(name) => crate::providers::provider_display_name(name).to_string(),
            None => "Provider".to_string(),
        };
        lines.push(section_header(&header));

        for (key, value) in &host.provider_meta {
            let label = meta_label(key);
            push_field(&mut lines, &label, value, max_value_width);
        }
    }

    // Tunnels section
    let tunnel_active = app.active_tunnels.contains_key(&host.alias);
    if host.tunnel_count > 0 {
        lines.push(Line::from(""));
        let tunnel_label = if tunnel_active {
            "Tunnels (active)"
        } else {
            "Tunnels"
        };
        lines.push(section_header(tunnel_label));

        let rules = find_tunnel_rules(&app.config.elements, &host.alias);
        let style = if tunnel_active {
            theme::bold()
        } else {
            theme::muted()
        };
        for rule in rules.iter().take(5) {
            lines.push(Line::from(Span::styled(rule.to_string(), style)));
        }
        if rules.len() > 5 {
            lines.push(Line::from(Span::styled(
                format!("(and {} more. T to manage)", rules.len() - 5),
                theme::muted(),
            )));
        }
    }

    // Snippets hint
    let snippet_count = app.snippet_store.snippets.len();
    if snippet_count > 0 {
        lines.push(Line::from(""));
        lines.push(section_header("Snippets"));
        lines.push(Line::from(Span::styled(
            format!("{} available (r to run)", snippet_count),
            theme::muted(),
        )));
    }

    // Containers section (only shown when cache data exists)
    if let Some(cache_entry) = app.container_cache.get(&host.alias) {
        lines.push(Line::from(""));
        lines.push(section_header("Containers"));
        let running = cache_entry
            .containers
            .iter()
            .filter(|c| c.state == "running")
            .count();
        let total = cache_entry.containers.len();
        push_field(
            &mut lines,
            "Total",
            &format!("{} running / {} total", running, total),
            max_value_width,
        );
        push_field(
            &mut lines,
            "Runtime",
            cache_entry.runtime.as_str(),
            max_value_width,
        );
        push_field(
            &mut lines,
            "Last checked",
            &crate::containers::format_relative_time(cache_entry.timestamp),
            max_value_width,
        );
        for container in cache_entry.containers.iter().take(5) {
            let (icon, icon_style) = match container.state.as_str() {
                "running" => ("\u{2713}", theme::success()),
                "exited" | "dead" => ("\u{2717}", theme::error()),
                _ => ("\u{25cf}", theme::bold()),
            };
            let name = crate::containers::truncate_str(
                &container.names,
                max_value_width.saturating_sub(2),
            );
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:>width$}", "", width = LABEL_WIDTH),
                    theme::muted(),
                ),
                Span::styled(icon, icon_style),
                Span::styled(" ", theme::muted()),
                Span::styled(name, theme::bold()),
            ]));
        }
        if total > 5 {
            lines.push(Line::from(Span::styled(
                format!("(and {} more. C to manage)", total - 5),
                theme::muted(),
            )));
        }
    }

    // Source section (for included hosts)
    if let Some(ref source) = host.source_file {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<width$}", "Source", width = LABEL_WIDTH),
                theme::muted(),
            ),
            Span::styled(
                super::truncate(&source.display().to_string(), max_value_width),
                theme::bold(),
            ),
        ]));
    }

    lines.push(Line::from(""));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((app.ui.detail_scroll, 0));
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

fn push_field(lines: &mut Vec<Line<'static>>, label: &str, value: &str, max_value_width: usize) {
    let display = if max_value_width > 0 {
        super::truncate(value, max_value_width)
    } else {
        value.to_string()
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!("{:<width$}", label, width = LABEL_WIDTH),
            theme::muted(),
        ),
        Span::styled(display, theme::bold()),
    ]));
}

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

fn section_header(label: &str) -> Line<'static> {
    Line::from(Span::styled(label.to_string(), theme::section_header()))
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
        names.iter().map(|n| format!("#{}", n)).collect()
    }

    #[test]
    fn wrap_tags_single_row() {
        let t = tags(&["prod", "web"]);
        let rows = wrap_tags(&t, 32);
        assert_eq!(rows, vec![vec!["#prod", "#web"]]);
    }

    #[test]
    fn wrap_tags_wraps_to_second_row() {
        let t = tags(&["production", "web", "europe", "api"]);
        // "#production #web" = 16 cols, "#europe" would make 24 > 20
        let rows = wrap_tags(&t, 20);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["#production", "#web"]);
        assert_eq!(rows[1], vec!["#europe", "#api"]);
    }

    #[test]
    fn wrap_tags_one_per_row_when_narrow() {
        let t = tags(&["production", "staging"]);
        // Each tag is 11 chars, panel only 12 wide — no room for two
        let rows = wrap_tags(&t, 12);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec!["#production"]);
        assert_eq!(rows[1], vec!["#staging"]);
    }

    #[test]
    fn wrap_tags_empty() {
        let rows = wrap_tags(&[], 32);
        assert!(rows.is_empty());
    }

    #[test]
    fn wrap_tags_exact_fit() {
        let t = tags(&["ab", "cd"]);
        // "#ab #cd" = 7 cols
        let rows = wrap_tags(&t, 7);
        assert_eq!(rows, vec![vec!["#ab", "#cd"]]);
    }

    #[test]
    fn wrap_tags_exact_overflow() {
        let t = tags(&["ab", "cd"]);
        // "#ab #cd" = 7 cols, max 6 → wraps
        let rows = wrap_tags(&t, 6);
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
}
