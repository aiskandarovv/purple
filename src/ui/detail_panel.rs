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

    if !host.proxy_jump.is_empty() {
        push_field(&mut lines, "ProxyJump", &host.proxy_jump, max_value_width);
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

    // Activity section
    let history_entry = app.history.entries.get(&host.alias);
    let ping = app.ping_status.get(&host.alias);

    if history_entry.is_some() || ping.is_some() {
        lines.push(Line::from(""));
        lines.push(section_header("Activity"));

        if let Some(entry) = history_entry {
            let ago = ConnectionHistory::format_time_ago(entry.last_connected);
            if !ago.is_empty() {
                push_field(&mut lines, "Last SSH", &ago, max_value_width);
            }
            push_field(
                &mut lines,
                "Connections",
                &entry.count.to_string(),
                max_value_width,
            );

            if !entry.timestamps.is_empty() && inner_width >= 10 {
                let chart_lines = activity_sparkline(&entry.timestamps, inner_width);
                if !chart_lines.is_empty() {
                    lines.push(Line::from(""));
                    lines.extend(chart_lines);
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

    // Tags section
    if !host.tags.is_empty() || host.provider.is_some() {
        lines.push(Line::from(""));
        lines.push(section_header("Tags"));

        let mut all_tags: Vec<String> = host.tags.iter().map(|t| format!("#{}", t)).collect();
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
                format!("(and {} more...)", rules.len() - 5),
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

    // Source section (for included hosts)
    if let Some(ref source) = host.source_file {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<width$}", "Source", width = LABEL_WIDTH),
                theme::muted(),
            ),
            Span::styled(source.display().to_string(), theme::muted()),
        ]));
    }

    lines.push(Line::from(""));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((app.ui.detail_scroll, 0));
    frame.render_widget(paragraph, area);
}

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
// 2 rows tall = 16 height levels. Each character = ~2 days over 12 weeks.
// History retains 90 days of timestamps; chart shows 84 (12 clean weeks).
const BLOCKS: [char; 9] = [
    ' ', '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
    '\u{2588}',
];
const CHART_DAYS: u64 = 84;

fn activity_sparkline(timestamps: &[u64], chart_width: usize) -> Vec<Line<'static>> {
    if chart_width == 0 {
        return Vec::new();
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let range_secs = CHART_DAYS * 86400;
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

    // Bottom row
    let mut bottom = String::with_capacity(chart_width * 3);
    for &h in &heights {
        if h == 0 {
            bottom.push(' ');
        } else if h >= 8 {
            bottom.push(BLOCKS[8]);
        } else {
            bottom.push(BLOCKS[h]);
        }
    }
    chart_lines.push(Line::from(Span::styled(bottom, theme::bold())));

    // Axis labels
    let left_label = format!("{}w", CHART_DAYS / 7);
    let right_label = "now";
    let gap = chart_width.saturating_sub(left_label.len() + right_label.len());
    chart_lines.push(Line::from(vec![
        Span::styled(left_label, theme::muted()),
        Span::raw(" ".repeat(gap)),
        Span::styled(right_label.to_string(), theme::muted()),
    ]));

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
        let old = now() - 100 * 86400;
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
        let ts = now() - 86400;
        let lines = activity_sparkline(&[ts], 30);
        let axis = lines.last().unwrap();
        let text: String = axis.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("12w"));
        assert!(text.contains("now"));
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
}
