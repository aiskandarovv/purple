use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use super::theme;
use crate::app::App;

pub fn render(frame: &mut Frame, app: &mut App) {
    let col1 = left_column();
    let col2 = middle_column();
    let col3 = right_column();
    let total_lines = col1.len().max(col2.len()).max(col3.len()) as u16;
    let width: u16 = 110.min(frame.area().width.saturating_sub(4));
    // 2 border + 1 footer
    let max_body = frame.area().height.saturating_sub(5);
    // 2 border + 1 footer + 1 padding above footer + 1 padding below footer
    let height = (total_lines + 5).min(frame.area().height.saturating_sub(2));
    let area = super::centered_rect_fixed(width, height, frame.area());

    frame.render_widget(Clear, area);

    let title = Span::styled(" Cheat Sheet ", theme::brand());
    let author = Line::from(Span::styled(
        " github.com/erickochen/purple ",
        theme::muted(),
    ));
    let version = Line::from(vec![
        Span::styled(
            format!(" v{}", env!("CARGO_PKG_VERSION")),
            theme::version(),
        ),
        Span::styled(
            format!(" (built {}) ", env!("PURPLE_BUILD_DATE")),
            theme::muted(),
        ),
    ]);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .title_bottom(author)
        .title_bottom(version.right_aligned())
        .border_style(theme::accent());

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1), // footer
        Constraint::Length(1), // padding before bottom border
    ])
    .split(inner);

    let cols = Layout::horizontal([
        Constraint::Length(36),
        Constraint::Length(36),
        Constraint::Min(0),
    ])
    .split(rows[0]);

    // Clamp scroll offset
    let max_scroll = total_lines.saturating_sub(max_body);
    if app.ui.help_scroll > max_scroll {
        app.ui.help_scroll = max_scroll;
    }

    let para1 = Paragraph::new(col1).scroll((app.ui.help_scroll, 0));
    let para2 = Paragraph::new(col2).scroll((app.ui.help_scroll, 0));
    let para3 = Paragraph::new(col3).scroll((app.ui.help_scroll, 0));
    frame.render_widget(para1, cols[0]);
    frame.render_widget(para2, cols[1]);
    frame.render_widget(para3, cols[2]);

    let can_scroll = total_lines > max_body;
    let spans = if can_scroll {
        vec![
            Span::styled(" j/k", theme::accent_bold()),
            Span::styled(" scroll ", theme::muted()),
            Span::styled("\u{2502} ", theme::muted()),
            Span::styled("Esc", theme::accent_bold()),
            Span::styled(" close", theme::muted()),
        ]
    } else {
        vec![
            Span::styled(" Esc", theme::accent_bold()),
            Span::styled(" close", theme::muted()),
        ]
    };
    frame.render_widget(Paragraph::new(Line::from(spans)), rows[1]);
}

fn left_column() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        section_header("NAVIGATION"),
        help_line(" j/k       ", "up / down"),
        help_line(" PgDn/PgUp ", "page down / up"),
        help_line(" Enter     ", "connect to host"),
        help_line(" /         ", "search hosts"),
        help_line(" #         ", "filter by tag"),
        help_line(" s         ", "cycle sort mode"),
        help_line(" g         ", "group by provider"),
        help_line(" v         ", "toggle detail panel"),
        Line::from(""),
        section_header("SEARCH SYNTAX"),
        help_line(" tag:name  ", "fuzzy tag filter"),
        help_line(" tag=name  ", "exact tag filter"),
        Line::from(""),
        section_header("FORMS"),
        help_line(" Tab       ", "next field"),
        help_line(" Shift+Tab ", "previous field"),
        help_line(" Enter     ", "save / open picker"),
        help_line(" ^D        ", "set global default"),
        help_line(" Esc       ", "cancel"),
    ]
}

fn middle_column() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        section_header("HOSTS"),
        help_line(" a         ", "add host"),
        help_line(" e         ", "edit host"),
        help_line(" d         ", "delete host"),
        help_line(" c         ", "clone host"),
        help_line(" t         ", "tag host (inline)"),
        help_line(" u         ", "undo delete"),
        help_line(" i         ", "inspect directives"),
        help_line(" y         ", "copy ssh command"),
        help_line(" x         ", "copy config block"),
        help_line(" p / P     ", "ping host / all"),
        help_line(" ^Space    ", "select / deselect"),
        help_line(" r         ", "run snippet on host(s)"),
        help_line(" R         ", "run on all visible"),
        help_line(" f         ", "remote file explorer"),
        help_line(" T         ", "tunnels for host"),
        help_line(" S         ", "cloud providers"),
        help_line(" K         ", "SSH keys"),
    ]
}

fn right_column() -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        section_header("FILE EXPLORER (f)"),
        help_line(" Tab       ", "switch pane"),
        help_line(" j/k       ", "navigate"),
        help_line(" Enter     ", "open dir / copy"),
        help_line(" Backspace ", "go up"),
        help_line(" ^Space    ", "select / deselect"),
        help_line(" ^A        ", "select all / none"),
        help_line(" .         ", "toggle hidden files"),
        help_line(" s         ", "cycle sort mode"),
        help_line(" R         ", "refresh both panes"),
        Line::from(""),
        Line::from(""),
        Line::from(""),
        Line::from(""),
        Line::from(""),
        Line::from(""),
        Line::from(""),
        help_line(" q / Esc   ", "quit / close"),
    ]
}

fn section_header(label: &str) -> Line<'static> {
    Line::from(Span::styled(format!(" {}", label), theme::muted()))
}

fn help_line<'a>(key: &'a str, desc: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(key, theme::accent_bold()),
        Span::raw(desc),
    ])
}
