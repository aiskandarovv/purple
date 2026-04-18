//! Screen enum: tags the currently-displayed overlay or view.

use std::path::PathBuf;

/// State for the What's New overlay.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct WhatsNewState {
    pub scroll: u16,
}

/// Which screen is currently displayed.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    HostList,
    AddHost,
    EditHost {
        alias: String,
    },
    ConfirmDelete {
        alias: String,
    },
    Help {
        return_screen: Box<Screen>,
    },
    KeyList,
    KeyDetail {
        index: usize,
    },
    HostDetail {
        index: usize,
    },
    TagPicker,
    ThemePicker,
    Providers,
    ProviderForm {
        provider: String,
    },
    TunnelList {
        alias: String,
    },
    TunnelForm {
        alias: String,
        editing: Option<usize>,
    },
    SnippetPicker {
        target_aliases: Vec<String>,
    },
    SnippetForm {
        target_aliases: Vec<String>,
        editing: Option<usize>,
    },
    SnippetOutput {
        snippet_name: String,
        target_aliases: Vec<String>,
    },
    SnippetParamForm {
        snippet: crate::snippet::Snippet,
        target_aliases: Vec<String>,
    },
    ConfirmHostKeyReset {
        alias: String,
        hostname: String,
        known_hosts_path: String,
        askpass: Option<String>,
    },
    FileBrowser {
        alias: String,
    },
    Containers {
        alias: String,
    },
    ConfirmImport {
        count: usize,
    },
    ConfirmPurgeStale {
        aliases: Vec<String>,
        provider: Option<String>,
    },
    ConfirmVaultSign {
        /// Precomputed list of (alias, role, certificate_file, pubkey_path) for
        /// hosts that resolve to a vault SSH role. Computed when the user
        /// presses `V`. `certificate_file` is the host's existing
        /// `CertificateFile` directive (empty when unset) and is needed so the
        /// background worker checks renewal status against the actually
        /// configured cert path rather than purple's default.
        signable: Vec<(String, String, String, PathBuf, Option<String>)>,
    },
    Welcome {
        has_backup: bool,
        host_count: usize,
        known_hosts_count: usize,
    },
    /// Bulk tag editor: tri-state checkbox picker that edits tags across
    /// all hosts in `multi_select` in one go. Opened via `t` when a
    /// multi-host selection is active.
    BulkTagEditor,
    /// What's New overlay: shows recent changelog sections to the user
    /// after an upgrade. Opened via the upgrade toast or `n` key.
    WhatsNew(WhatsNewState),
}

impl Screen {
    /// Stable short variant name used in state-transition logs.
    /// Omits inner fields so log lines never leak host aliases, paths or
    /// tokens.
    pub fn variant_name(&self) -> &'static str {
        match self {
            Screen::HostList => "HostList",
            Screen::AddHost => "AddHost",
            Screen::EditHost { .. } => "EditHost",
            Screen::ConfirmDelete { .. } => "ConfirmDelete",
            Screen::Help { .. } => "Help",
            Screen::KeyList => "KeyList",
            Screen::KeyDetail { .. } => "KeyDetail",
            Screen::HostDetail { .. } => "HostDetail",
            Screen::TagPicker => "TagPicker",
            Screen::ThemePicker => "ThemePicker",
            Screen::Providers => "Providers",
            Screen::ProviderForm { .. } => "ProviderForm",
            Screen::TunnelList { .. } => "TunnelList",
            Screen::TunnelForm { .. } => "TunnelForm",
            Screen::SnippetPicker { .. } => "SnippetPicker",
            Screen::SnippetForm { .. } => "SnippetForm",
            Screen::SnippetOutput { .. } => "SnippetOutput",
            Screen::SnippetParamForm { .. } => "SnippetParamForm",
            Screen::ConfirmHostKeyReset { .. } => "ConfirmHostKeyReset",
            Screen::FileBrowser { .. } => "FileBrowser",
            Screen::Containers { .. } => "Containers",
            Screen::ConfirmImport { .. } => "ConfirmImport",
            Screen::ConfirmPurgeStale { .. } => "ConfirmPurgeStale",
            Screen::ConfirmVaultSign { .. } => "ConfirmVaultSign",
            Screen::Welcome { .. } => "Welcome",
            Screen::BulkTagEditor => "BulkTagEditor",
            Screen::WhatsNew(_) => "WhatsNew",
        }
    }
}
