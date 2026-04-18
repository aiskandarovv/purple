//! Form baselines and dirty-state detection. Implements `impl App` continuation
//! with capture/compare logic for every form kind (host, tunnel, snippet,
//! provider) plus the mtime helpers that detect external config changes.

use std::path::PathBuf;
use std::time::SystemTime;

use super::{FormBaseline, ProviderFormBaseline, SnippetFormBaseline, TunnelFormBaseline};
use crate::app::App;
use crate::ssh_config::model::SshConfigFile;

impl App {
    /// Clear form mtime state (call on form cancel or successful submit).
    pub fn clear_form_mtime(&mut self) {
        self.conflict.form_mtime = None;
        self.conflict.form_include_mtimes.clear();
        self.conflict.form_include_dir_mtimes.clear();
        self.conflict.provider_form_mtime = None;
    }

    /// Capture config and Include file mtimes when opening a host form.
    pub fn capture_form_mtime(&mut self) {
        self.conflict.form_mtime = Self::get_mtime(&self.reload.config_path);
        self.conflict.form_include_mtimes =
            Self::snapshot_include_mtimes(&self.hosts_state.ssh_config);
        self.conflict.form_include_dir_mtimes =
            Self::snapshot_include_dir_mtimes(&self.hosts_state.ssh_config);
    }

    /// Capture ~/.purple/providers mtime when opening a provider form.
    pub fn capture_provider_form_mtime(&mut self) {
        let path = dirs::home_dir().map(|h| h.join(".purple/providers"));
        self.conflict.provider_form_mtime = path.as_ref().and_then(|p| Self::get_mtime(p));
    }

    /// Capture a baseline snapshot of the host form for dirty-check on Esc.
    pub fn capture_form_baseline(&mut self) {
        self.forms.host_baseline = Some(FormBaseline {
            alias: self.forms.host.alias.clone(),
            hostname: self.forms.host.hostname.clone(),
            user: self.forms.host.user.clone(),
            port: self.forms.host.port.clone(),
            identity_file: self.forms.host.identity_file.clone(),
            proxy_jump: self.forms.host.proxy_jump.clone(),
            askpass: self.forms.host.askpass.clone(),
            vault_ssh: self.forms.host.vault_ssh.clone(),
            vault_addr: self.forms.host.vault_addr.clone(),
            tags: self.forms.host.tags.clone(),
        });
    }

    /// Check if the host form has been modified since baseline was captured.
    pub fn host_form_is_dirty(&self) -> bool {
        match &self.forms.host_baseline {
            Some(b) => {
                self.forms.host.alias != b.alias
                    || self.forms.host.hostname != b.hostname
                    || self.forms.host.user != b.user
                    || self.forms.host.port != b.port
                    || self.forms.host.identity_file != b.identity_file
                    || self.forms.host.proxy_jump != b.proxy_jump
                    || self.forms.host.askpass != b.askpass
                    || self.forms.host.vault_ssh != b.vault_ssh
                    || self.forms.host.vault_addr != b.vault_addr
                    || self.forms.host.tags != b.tags
            }
            None => false,
        }
    }

    /// Capture a baseline snapshot of the tunnel form for dirty-check on Esc.
    pub fn capture_tunnel_form_baseline(&mut self) {
        self.tunnels.form_baseline = Some(TunnelFormBaseline {
            tunnel_type: self.tunnels.form.tunnel_type,
            bind_port: self.tunnels.form.bind_port.clone(),
            remote_host: self.tunnels.form.remote_host.clone(),
            remote_port: self.tunnels.form.remote_port.clone(),
            bind_address: self.tunnels.form.bind_address.clone(),
        });
    }

    /// Check if the tunnel form has been modified since baseline was captured.
    pub fn tunnel_form_is_dirty(&self) -> bool {
        match &self.tunnels.form_baseline {
            Some(b) => {
                self.tunnels.form.tunnel_type != b.tunnel_type
                    || self.tunnels.form.bind_port != b.bind_port
                    || self.tunnels.form.remote_host != b.remote_host
                    || self.tunnels.form.remote_port != b.remote_port
                    || self.tunnels.form.bind_address != b.bind_address
            }
            None => false,
        }
    }

    /// Capture a baseline snapshot of the snippet form for dirty-check on Esc.
    pub fn capture_snippet_form_baseline(&mut self) {
        self.snippets.form_baseline = Some(SnippetFormBaseline {
            name: self.snippets.form.name.clone(),
            command: self.snippets.form.command.clone(),
            description: self.snippets.form.description.clone(),
        });
    }

    /// Check if the snippet form has been modified since baseline was captured.
    pub fn snippet_form_is_dirty(&self) -> bool {
        match &self.snippets.form_baseline {
            Some(b) => {
                self.snippets.form.name != b.name
                    || self.snippets.form.command != b.command
                    || self.snippets.form.description != b.description
            }
            None => false,
        }
    }

    /// Capture a baseline snapshot of the provider form for dirty-check on Esc.
    pub fn capture_provider_form_baseline(&mut self) {
        self.providers.form_baseline = Some(ProviderFormBaseline {
            url: self.providers.form.url.clone(),
            token: self.providers.form.token.clone(),
            profile: self.providers.form.profile.clone(),
            project: self.providers.form.project.clone(),
            compartment: self.providers.form.compartment.clone(),
            regions: self.providers.form.regions.clone(),
            alias_prefix: self.providers.form.alias_prefix.clone(),
            user: self.providers.form.user.clone(),
            identity_file: self.providers.form.identity_file.clone(),
            verify_tls: self.providers.form.verify_tls,
            auto_sync: self.providers.form.auto_sync,
            vault_role: self.providers.form.vault_role.clone(),
            vault_addr: self.providers.form.vault_addr.clone(),
        });
    }

    /// Check if the provider form has been modified since baseline was captured.
    pub fn provider_form_is_dirty(&self) -> bool {
        match &self.providers.form_baseline {
            Some(b) => {
                self.providers.form.url != b.url
                    || self.providers.form.token != b.token
                    || self.providers.form.profile != b.profile
                    || self.providers.form.project != b.project
                    || self.providers.form.compartment != b.compartment
                    || self.providers.form.regions != b.regions
                    || self.providers.form.alias_prefix != b.alias_prefix
                    || self.providers.form.user != b.user
                    || self.providers.form.identity_file != b.identity_file
                    || self.providers.form.verify_tls != b.verify_tls
                    || self.providers.form.auto_sync != b.auto_sync
                    || self.providers.form.vault_role != b.vault_role
                    || self.providers.form.vault_addr != b.vault_addr
            }
            None => false,
        }
    }

    /// Check if config or any Include file/directory has changed since the form was opened.
    pub fn config_changed_since_form_open(&self) -> bool {
        match self.conflict.form_mtime {
            Some(open_mtime) => {
                if Self::get_mtime(&self.reload.config_path) != Some(open_mtime) {
                    return true;
                }
                self.conflict
                    .form_include_mtimes
                    .iter()
                    .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
                    || self
                        .conflict
                        .form_include_dir_mtimes
                        .iter()
                        .any(|(path, old_mtime)| Self::get_mtime(path) != *old_mtime)
            }
            None => false,
        }
    }

    /// Check if ~/.purple/providers has changed since the provider form was opened.
    pub fn provider_config_changed_since_form_open(&self) -> bool {
        let path = dirs::home_dir().map(|h| h.join(".purple/providers"));
        let current_mtime = path.as_ref().and_then(|p| Self::get_mtime(p));
        self.conflict.provider_form_mtime != current_mtime
    }

    /// Snapshot mtimes of all resolved Include files.
    pub(super) fn snapshot_include_mtimes(
        config: &SshConfigFile,
    ) -> Vec<(PathBuf, Option<SystemTime>)> {
        config
            .include_paths()
            .into_iter()
            .map(|p| {
                let mtime = Self::get_mtime(&p);
                (p, mtime)
            })
            .collect()
    }

    /// Snapshot mtimes of parent directories of Include glob patterns.
    pub(super) fn snapshot_include_dir_mtimes(
        config: &SshConfigFile,
    ) -> Vec<(PathBuf, Option<SystemTime>)> {
        config
            .include_glob_dirs()
            .into_iter()
            .map(|p| {
                let mtime = Self::get_mtime(&p);
                (p, mtime)
            })
            .collect()
    }
}
