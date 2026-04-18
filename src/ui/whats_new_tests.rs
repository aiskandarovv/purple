use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::animation::AnimationState;
use crate::app::{App, Screen, WhatsNewState};
use crate::changelog;

fn test_override_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn build_app() -> App {
    let path = tempfile::tempdir()
        .expect("tempdir")
        .keep()
        .join("test_config");
    let config = crate::ssh_config::model::SshConfigFile {
        elements: crate::ssh_config::model::SshConfigFile::parse_content(""),
        path,
        crlf: false,
        bom: false,
    };
    let mut app = App::new(config);
    app.providers.config = crate::providers::config::ProviderConfig::default();
    app
}

fn render_with_fixture(width: u16, height: u16, scroll: u16, fixture_path: &str) -> String {
    let _guard = test_override_lock();
    let mut captured = String::new();
    crate::preferences::tests_helpers::with_temp_prefs("whats_new_render", |_| {
        crate::preferences::save_last_seen_version("0.0.1").unwrap();
        crate::ui::theme::init_with_mode(1);
        let mut app = build_app();
        app.screen = Screen::WhatsNew(WhatsNewState { scroll });
        let fixture = std::fs::read_to_string(fixture_path).unwrap();
        changelog::set_test_override(fixture);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut anim = AnimationState::default();
        terminal
            .draw(|f| crate::ui::render(f, &mut app, &mut anim))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        for y in 0..height {
            for x in 0..width {
                captured.push_str(buffer[(x, y)].symbol());
            }
            captured.push('\n');
        }
        changelog::clear_test_override();
    });
    captured
}

#[test]
fn renders_title() {
    let out = render_with_fixture(120, 40, 0, "tests/fixtures/changelog/simple.md");
    assert!(out.contains("What's new"), "title missing, got:\n{out}");
}

#[test]
fn renders_at_minimum_terminal_size_without_truncation() {
    let out = render_with_fixture(80, 24, 0, "tests/fixtures/changelog/simple.md");
    assert!(out.contains("What's new"), "title missing, got:\n{out}");
    assert!(out.contains("close"), "close action missing, got:\n{out}");
}

#[test]
fn renders_strict_glyph_prefixes() {
    let out = render_with_fixture(120, 40, 0, "tests/fixtures/changelog/simple.md");
    assert!(out.contains("+ feat"), "feat prefix missing, got:\n{out}");
    assert!(out.contains("! fix"), "fix prefix missing, got:\n{out}");
}

#[test]
fn renders_scroll_indicator_when_content_overflows() {
    let _guard = test_override_lock();
    crate::ui::theme::init_with_mode(1);
    let mut app = build_app();
    app.screen = Screen::WhatsNew(WhatsNewState { scroll: 5 });

    changelog::set_test_override("## 1.0.0\n- feat: a\n- feat: b\n- feat: c\n".into());
    let backend = TestBackend::new(120, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut anim = AnimationState::default();
    terminal
        .draw(|f| crate::ui::render(f, &mut app, &mut anim))
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..20 {
        for x in 0..120 {
            out.push_str(buf[(x, y)].symbol());
        }
    }
    changelog::clear_test_override();
    assert!(
        out.contains('/'),
        "scroll indicator '/' missing, got:\n{out}"
    );
}
