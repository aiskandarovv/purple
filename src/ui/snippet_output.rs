use ratatui::Frame;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::design;
use super::theme;
use crate::app::{App, Screen};

pub fn render(frame: &mut Frame, app: &mut App) {
    let (snippet_name, host_count) = match &app.screen {
        Screen::SnippetOutput {
            snippet_name,
            target_aliases,
        } => (snippet_name.clone(), target_aliases.len()),
        _ => return,
    };

    let state = match &app.snippet_output {
        Some(s) => s,
        None => return,
    };

    let area = design::overlay_area(frame, 90, 85, frame.area().height);
    frame.render_widget(Clear, area);

    // Title with progress
    let host_word = if host_count == 1 { "host" } else { "hosts" };
    let title = if state.all_done {
        format!("Ran '{}' on {} {}", snippet_name, host_count, host_word)
    } else {
        format!(
            "Running '{}' ({}/{} {})",
            snippet_name, state.completed, state.total, host_word
        )
    };

    let block = design::overlay_block(&title);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (content, footer) = design::content_and_footer(inner);

    let width = content.width as usize;

    // Build all lines from results
    let mut lines: Vec<Line<'_>> = Vec::new();

    if state.results.is_empty() {
        let msg = if state.all_done {
            "  [No results]"
        } else {
            "  Running..."
        };
        lines.push(Line::from(Span::styled(msg, theme::muted())));
    }

    for result in &state.results {
        // Host header with exit code
        let status_text = match result.exit_code {
            Some(0) => " \u{2713}".to_string(),
            Some(code) => format!(" exit {}", code),
            None => " error".to_string(),
        };
        let status_style = match result.exit_code {
            Some(0) => theme::success(),
            _ => theme::error(),
        };

        let prefix = format!("  \u{2500}\u{2500} {} ", result.alias);
        let used = prefix.width() + status_text.width() + 1;
        let fill = width.saturating_sub(used);

        lines.push(Line::from(vec![
            Span::styled(prefix, theme::bold()),
            Span::styled(status_text, status_style),
            Span::styled(format!(" {}", "\u{2500}".repeat(fill)), theme::border()),
        ]));

        if result.stdout.is_empty() && result.stderr.is_empty() {
            lines.push(Line::from(Span::styled("  [No output]", theme::muted())));
        } else {
            for line in result.stdout.lines() {
                lines.push(Line::from(Span::raw(format!("  {}", line))));
            }
            for line in result.stderr.lines() {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    theme::error(),
                )));
            }
        }
        lines.push(Line::from(""));
    }

    // Offset-based rendering: slice to visible window (no u16 limit)
    let visible_height = content.height as usize;
    let total = lines.len();
    let max_offset = total.saturating_sub(visible_height);
    let offset = state.scroll_offset.min(max_offset);
    let visible: Vec<Line<'_>> = lines
        .into_iter()
        .skip(offset)
        .take(visible_height)
        .collect();

    frame.render_widget(Paragraph::new(visible), content);

    // Footer
    let mut f = design::Footer::new();
    if state.all_done {
        f = f.action("Esc", " close ").action("c", " copy ");
    } else {
        f = f.action("Ctrl+C", " cancel ");
    }
    f = f
        .action("j/k", " scroll ")
        .action("n/N", " next/prev host ")
        .action("g/G", " top/bottom");
    f.render_with_status(frame, footer, app);
}

#[cfg(test)]
mod tests {
    use ratatui::layout::{Constraint, Layout, Rect};

    #[test]
    fn layout_has_spacer_between_content_and_footer() {
        let area = Rect::new(0, 0, 80, 30);
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
        assert_eq!(chunks[1].height, 1);
        assert_eq!(chunks[2].height, 1);
        assert!(chunks[2].y > chunks[0].y + chunks[0].height);
    }
}
