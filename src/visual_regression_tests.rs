//! Visual regression tests for every screen.
//!
//! Each test renders one screen into a `TestBackend` buffer using demo data,
//! serialises the buffer (characters plus per-cell style info) and compares
//! the result against a `.golden` baseline in `tests/visual_golden/`. Any
//! visual change to spacing, colors, text or borders fails the test.
//!
//! Regenerate baselines after intentional UI changes:
//!     ./scripts/update-golden.sh
//!
//! Implementation notes:
//! - Tests live in the binary crate (not in `tests/`) because they need
//!   access to private types (`App`, `ui::render`, `animation::AnimationState`).
//! - All tests pin the color mode to ANSI 16 (`init_with_mode(1)`) so output
//!   is deterministic across terminals (no truecolor RGB drift, no NO_COLOR
//!   stripping).
//! - Tests use a process-wide lock to serialise demo state mutations and
//!   theme initialisation across `cargo test` worker threads.

use std::path::PathBuf;
use std::sync::MutexGuard;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use crate::animation::AnimationState;
use crate::app::{App, Screen};
use crate::demo;
use crate::demo_flag;
use crate::preferences;
use crate::ui;

const TERM_WIDTH: u16 = 100;
const TERM_HEIGHT: u16 = 30;

/// RAII guard returned by `setup()`. Holds the cross-suite lock for the
/// duration of the test, resets the demo flag on drop so subsequent
/// non-visual tests do not observe a sticky `demo_flag::is_demo() == true`,
/// and clears the preferences path override so later tests do not inherit
/// a stale thread-local pointer.
struct VisualGuard {
    _lock: MutexGuard<'static, ()>,
}

impl Drop for VisualGuard {
    fn drop(&mut self) {
        demo_flag::disable();
        preferences::clear_path_override_for_tests();
    }
}

/// Acquire the cross-suite test lock, pin ANSI 16 colors, point the
/// preferences path at a non-existent file so reads (e.g. last_seen_version
/// consumed by the What's New overlay) return `None` regardless of the host
/// environment, and return a guard that releases the lock and resets the
/// demo flag on drop.
///
/// The lock is shared with `preferences::tests::with_temp_prefs` because both
/// suites mutate process-wide state (`PATH_OVERRIDE`, `demo_flag::DEMO_MODE`)
/// that would otherwise race when `cargo test` runs them concurrently.
///
/// Env-sensitivity audit: visual tests must be byte-identical on any host.
/// The consumed state is:
/// - `ui::theme` — pinned via `init_with_mode(1)`, ignores NO_COLOR/COLORTERM
/// - `preferences` — path_override below, so ~/.purple/preferences is ignored
/// - `CHANGELOG.md` — embedded via `include_str!` at compile time
/// - `CARGO_PKG_VERSION` / `PURPLE_BUILD_DATE` — compile-time env vars
///   (build date drifts by calendar day; accepted as known limitation)
#[must_use]
fn setup() -> VisualGuard {
    let lock = preferences::GLOBAL_TEST_IO_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    ui::theme::init_with_mode(1);
    // Point at a path that does not exist so load_* returns None. We
    // intentionally do NOT create the file — individual tests may override
    // this if they need canned preference values.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let sentinel = std::env::temp_dir().join(format!(
        "purple_vistest_nonexistent_{}_{}",
        std::process::id(),
        nanos,
    ));
    preferences::set_path_override(sentinel);
    VisualGuard { _lock: lock }
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/visual_golden")
}

fn golden_path(name: &str) -> PathBuf {
    golden_dir().join(format!("{name}.golden"))
}

fn color_name(c: Color) -> String {
    match c {
        Color::Reset => "Reset".into(),
        Color::Black => "Black".into(),
        Color::Red => "Red".into(),
        Color::Green => "Green".into(),
        Color::Yellow => "Yellow".into(),
        Color::Blue => "Blue".into(),
        Color::Magenta => "Magenta".into(),
        Color::Cyan => "Cyan".into(),
        Color::Gray => "Gray".into(),
        Color::DarkGray => "DarkGray".into(),
        Color::LightRed => "LightRed".into(),
        Color::LightGreen => "LightGreen".into(),
        Color::LightYellow => "LightYellow".into(),
        Color::LightBlue => "LightBlue".into(),
        Color::LightMagenta => "LightMagenta".into(),
        Color::LightCyan => "LightCyan".into(),
        Color::White => "White".into(),
        Color::Rgb(r, g, b) => format!("Rgb({r},{g},{b})"),
        Color::Indexed(i) => format!("Indexed({i})"),
    }
}

fn modifier_name(m: Modifier) -> String {
    if m.is_empty() {
        return "-".into();
    }
    let mut parts = Vec::new();
    if m.contains(Modifier::BOLD) {
        parts.push("BOLD");
    }
    if m.contains(Modifier::DIM) {
        parts.push("DIM");
    }
    if m.contains(Modifier::ITALIC) {
        parts.push("ITALIC");
    }
    if m.contains(Modifier::UNDERLINED) {
        parts.push("UNDERLINED");
    }
    if m.contains(Modifier::SLOW_BLINK) {
        parts.push("SLOW_BLINK");
    }
    if m.contains(Modifier::RAPID_BLINK) {
        parts.push("RAPID_BLINK");
    }
    if m.contains(Modifier::REVERSED) {
        parts.push("REVERSED");
    }
    if m.contains(Modifier::HIDDEN) {
        parts.push("HIDDEN");
    }
    if m.contains(Modifier::CROSSED_OUT) {
        parts.push("CROSSED_OUT");
    }
    parts.join("|")
}

/// Serialise a buffer to a deterministic string: a character grid followed by
/// a `---STYLES---` marker and one line per non-default cell with its style.
fn serialize_buffer(buf: &Buffer) -> String {
    let mut out = String::new();
    let area = buf.area;
    for y in 0..area.height {
        for x in 0..area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out.push_str("---STYLES---\n");
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buf[(x, y)];
            let is_default_fg = matches!(cell.fg, Color::Reset);
            let is_default_bg = matches!(cell.bg, Color::Reset);
            let is_default_mod = cell.modifier.is_empty();
            if is_default_fg && is_default_bg && is_default_mod {
                continue;
            }
            out.push_str(&format!(
                "({x},{y}) fg={} bg={} mod={}\n",
                color_name(cell.fg),
                color_name(cell.bg),
                modifier_name(cell.modifier),
            ));
        }
    }
    out
}

/// Compare actual output to the golden file. When `UPDATE_GOLDEN=1` is set,
/// overwrite the golden file instead of asserting.
fn assert_golden(name: &str, actual: &str) {
    let path = golden_path(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden_dir()).expect("create golden dir");
        std::fs::write(&path, actual).expect("write golden");
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read golden {}: {e}. Run UPDATE_GOLDEN=1 cargo test --bin purple visual_regression to create it.",
            path.display()
        )
    });

    if expected != actual {
        // Write the actual output next to the golden so the diff is easy to inspect.
        let actual_path = path.with_extension("actual");
        let _ = std::fs::write(&actual_path, actual);
        panic!(
            "visual regression: {name} differs from baseline.\n  golden: {}\n  actual: {}\nIf the change is intentional, run ./scripts/update-golden.sh and review the diff.",
            path.display(),
            actual_path.display(),
        );
    }
}

/// Render the given screen into a buffer and return the serialised result.
fn render_screen(app: &mut App) -> String {
    let backend = TestBackend::new(TERM_WIDTH, TERM_HEIGHT);
    let mut terminal = Terminal::new(backend).expect("create terminal");
    let mut anim = AnimationState::default();
    terminal
        .draw(|frame| ui::render(frame, app, &mut anim))
        .expect("render frame");
    let buf = terminal.backend().buffer().clone();
    serialize_buffer(&buf)
}

// ---------------------------------------------------------------------------
// Tests (29 total). Each test pins ANSI-16 colors, builds a fresh demo app,
// switches to the target screen, renders it and compares against a golden.
// ---------------------------------------------------------------------------

#[test]
fn visual_host_list() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    let actual = render_screen(&mut app);
    assert_golden("host_list", &actual);
}

#[test]
fn visual_host_list_search() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.start_search_with("aws");
    let actual = render_screen(&mut app);
    assert_golden("host_list_search", &actual);
}

#[test]
fn visual_host_list_detail_panel() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    // Detail panel renders alongside the host list when view_mode is Detailed
    // and the terminal is wide enough (DETAIL_MIN_WIDTH).
    app.hosts_state.view_mode = crate::app::ViewMode::Detailed;
    let actual = render_screen(&mut app);
    assert_golden("host_list_detail_panel", &actual);
}

#[test]
fn visual_host_form_add() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::AddHost;
    let actual = render_screen(&mut app);
    assert_golden("host_form_add", &actual);
}

#[test]
fn visual_host_form_edit() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::EditHost {
        alias: "bastion-ams".to_string(),
    };
    let actual = render_screen(&mut app);
    assert_golden("host_form_edit", &actual);
}

#[test]
fn visual_host_detail() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::HostDetail { index: 0 };
    let actual = render_screen(&mut app);
    assert_golden("host_detail", &actual);
}

#[test]
fn visual_tunnel_list() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::TunnelList {
        alias: "bastion-ams".to_string(),
    };
    let actual = render_screen(&mut app);
    assert_golden("tunnel_list", &actual);
}

#[test]
fn visual_tunnel_form() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::TunnelForm {
        alias: "bastion-ams".to_string(),
        editing: None,
    };
    let actual = render_screen(&mut app);
    assert_golden("tunnel_form", &actual);
}

#[test]
fn visual_key_list() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::KeyList;
    let actual = render_screen(&mut app);
    assert_golden("key_list", &actual);
}

#[test]
fn visual_key_detail() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::KeyDetail { index: 0 };
    let actual = render_screen(&mut app);
    assert_golden("key_detail", &actual);
}

#[test]
fn visual_help() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::Help {
        return_screen: Box::new(Screen::HostList),
    };
    let actual = render_screen(&mut app);
    assert_golden("help", &actual);
}

#[test]
fn visual_confirm_delete() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::ConfirmDelete {
        alias: "bastion-ams".to_string(),
    };
    let actual = render_screen(&mut app);
    assert_golden("confirm_delete", &actual);
}

#[test]
fn visual_snippet_picker() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::SnippetPicker {
        target_aliases: vec!["bastion-ams".to_string()],
    };
    let actual = render_screen(&mut app);
    assert_golden("snippet_picker", &actual);
}

#[test]
fn visual_snippet_form() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::SnippetForm {
        target_aliases: vec!["bastion-ams".to_string()],
        editing: None,
    };
    let actual = render_screen(&mut app);
    assert_golden("snippet_form", &actual);
}

#[test]
fn visual_snippet_output() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.snippets.output = Some(crate::app::SnippetOutputState {
        run_id: 1,
        results: vec![crate::app::SnippetHostOutput {
            alias: "bastion-ams".to_string(),
            stdout: "load average: 0.12 0.18 0.21\n".to_string(),
            stderr: String::new(),
            exit_code: Some(0),
        }],
        scroll_offset: 0,
        completed: 1,
        total: 1,
        all_done: true,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });
    app.screen = Screen::SnippetOutput {
        snippet_name: "uptime".to_string(),
        target_aliases: vec!["bastion-ams".to_string()],
    };
    let actual = render_screen(&mut app);
    assert_golden("snippet_output", &actual);
}

#[test]
fn visual_snippet_param_form() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    let snippet = crate::snippet::Snippet {
        name: "uptime".to_string(),
        command: "uptime".to_string(),
        description: "Server uptime and load".to_string(),
    };
    // Param form requires state populated with the snippet's params (none here),
    // so build an empty SnippetParamFormState matching the snippet.
    let params: Vec<crate::snippet::SnippetParam> = Vec::new();
    app.snippets.param_form = Some(crate::app::SnippetParamFormState::new(&params));
    app.screen = Screen::SnippetParamForm {
        snippet,
        target_aliases: vec!["bastion-ams".to_string()],
    };
    let actual = render_screen(&mut app);
    assert_golden("snippet_param_form", &actual);
}

#[test]
fn visual_tag_picker() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::TagPicker;
    let actual = render_screen(&mut app);
    assert_golden("tag_picker", &actual);
}

#[test]
fn visual_theme_picker() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.ui.theme_picker.builtins = ui::theme::ThemeDef::builtins();
    app.ui.theme_picker.custom = Vec::new();
    app.ui.theme_picker.saved_name = "Purple".to_string();
    app.ui.theme_picker.list.select(Some(0));
    app.screen = Screen::ThemePicker;
    let actual = render_screen(&mut app);
    assert_golden("theme_picker", &actual);
}

#[test]
fn visual_containers() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    // Containers screen requires container_state. Populate from the demo cache.
    let alias = "bastion-ams".to_string();
    let cached = app
        .container_cache
        .get(&alias)
        .map(|c| c.containers.clone())
        .unwrap_or_default();
    app.container_state = Some(crate::app::ContainerState {
        alias: alias.clone(),
        askpass: None,
        runtime: Some(crate::containers::ContainerRuntime::Docker),
        containers: cached,
        list_state: ratatui::widgets::ListState::default(),
        loading: false,
        error: None,
        action_in_progress: None,
        confirm_action: None,
    });
    app.screen = Screen::Containers { alias };
    let actual = render_screen(&mut app);
    assert_golden("containers", &actual);
}

#[test]
fn visual_file_browser() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    let alias = "bastion-ams".to_string();
    // Use a deterministic empty browser state. remote_loading=true skips remote
    // I/O and local entries are intentionally empty so output is host-agnostic.
    app.file_browser = Some(crate::file_browser::FileBrowserState {
        alias: alias.clone(),
        askpass: None,
        active_pane: crate::file_browser::BrowserPane::Local,
        local_path: std::path::PathBuf::from("/demo"),
        local_entries: Vec::new(),
        local_list_state: ratatui::widgets::ListState::default(),
        local_selected: std::collections::HashSet::new(),
        local_error: None,
        remote_path: String::new(),
        remote_entries: Vec::new(),
        remote_list_state: ratatui::widgets::ListState::default(),
        remote_selected: std::collections::HashSet::new(),
        remote_error: None,
        remote_loading: true,
        show_hidden: false,
        sort: crate::file_browser::BrowserSort::Name,
        confirm_copy: None,
        transferring: None,
        transfer_error: None,
        connection_recorded: true,
    });
    app.screen = Screen::FileBrowser { alias };
    let actual = render_screen(&mut app);
    assert_golden("file_browser", &actual);
}

#[test]
fn visual_command_palette() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.palette = Some(crate::app::CommandPaletteState::default());
    let actual = render_screen(&mut app);
    assert_golden("command_palette", &actual);
}

#[test]
fn visual_bulk_tag_editor() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    // Bulk tag editor operates on multi_select. Populate it with a couple of demo hosts.
    app.hosts_state.multi_select.insert(0);
    app.hosts_state.multi_select.insert(1);
    app.screen = Screen::BulkTagEditor;
    let actual = render_screen(&mut app);
    assert_golden("bulk_tag_editor", &actual);
}

#[test]
fn visual_provider_list() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::Providers;
    let actual = render_screen(&mut app);
    assert_golden("provider_list", &actual);
}

#[test]
fn visual_provider_form() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::ProviderForm {
        provider: "aws".to_string(),
    };
    let actual = render_screen(&mut app);
    assert_golden("provider_form", &actual);
}

#[test]
fn visual_confirm_host_key_reset() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::ConfirmHostKeyReset {
        alias: "bastion-ams".to_string(),
        hostname: "bastion.example.com".to_string(),
        known_hosts_path: "/demo/.ssh/known_hosts".to_string(),
        askpass: None,
    };
    let actual = render_screen(&mut app);
    assert_golden("confirm_host_key_reset", &actual);
}

#[test]
fn visual_confirm_import() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::ConfirmImport { count: 5 };
    let actual = render_screen(&mut app);
    assert_golden("confirm_import", &actual);
}

#[test]
fn visual_confirm_purge_stale() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::ConfirmPurgeStale {
        aliases: vec!["aws-old-1".to_string(), "aws-old-2".to_string()],
        provider: Some("aws".to_string()),
    };
    let actual = render_screen(&mut app);
    assert_golden("confirm_purge_stale", &actual);
}

#[test]
fn visual_confirm_vault_sign() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::ConfirmVaultSign {
        signable: Vec::new(),
    };
    let actual = render_screen(&mut app);
    assert_golden("confirm_vault_sign", &actual);
}

#[test]
fn visual_welcome() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::Welcome {
        has_backup: true,
        host_count: 22,
        known_hosts_count: 47,
    };
    let actual = render_screen(&mut app);
    assert_golden("welcome", &actual);
}

#[test]
fn visual_whats_new() {
    let _g = setup();
    let mut app = demo::build_demo_app();
    app.screen = Screen::WhatsNew(crate::app::WhatsNewState::default());
    let fixture = std::fs::read_to_string("tests/fixtures/changelog/simple.md").unwrap();
    crate::changelog::set_test_override(fixture);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let actual = render_screen(&mut app);
        assert_golden("whats_new", &actual);
    }));
    crate::changelog::clear_test_override();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
