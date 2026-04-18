//! Auto-reload mtime tracking and form conflict mtimes.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::ssh_config::model::SshConfigFile;

/// Auto-reload mtime tracking.
#[derive(Default)]
pub struct ReloadState {
    pub config_path: PathBuf,
    pub last_modified: Option<SystemTime>,
    pub include_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
    pub include_dir_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
}

/// Form conflict detection mtimes.
#[derive(Default)]
pub struct ConflictState {
    pub form_mtime: Option<SystemTime>,
    pub form_include_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
    pub form_include_dir_mtimes: Vec<(PathBuf, Option<SystemTime>)>,
    pub provider_form_mtime: Option<SystemTime>,
}

impl ReloadState {
    /// Build from a loaded config: captures initial mtimes for the main file
    /// and every Include'd file and directory.
    pub fn from_config(config: &SshConfigFile) -> Self {
        let config_path = config.path.clone();
        let last_modified = get_mtime(&config_path);
        let include_mtimes = snapshot_include_mtimes(config);
        let include_dir_mtimes = snapshot_include_dir_mtimes(config);
        Self {
            config_path,
            last_modified,
            include_mtimes,
            include_dir_mtimes,
        }
    }
}

/// Get the modification time of a file.
pub fn get_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// Snapshot mtimes of all resolved Include files.
pub fn snapshot_include_mtimes(config: &SshConfigFile) -> Vec<(PathBuf, Option<SystemTime>)> {
    config
        .include_paths()
        .into_iter()
        .map(|p| {
            let mtime = get_mtime(&p);
            (p, mtime)
        })
        .collect()
}

/// Snapshot mtimes of parent directories of Include glob patterns.
pub fn snapshot_include_dir_mtimes(config: &SshConfigFile) -> Vec<(PathBuf, Option<SystemTime>)> {
    config
        .include_glob_dirs()
        .into_iter()
        .map(|p| {
            let mtime = get_mtime(&p);
            (p, mtime)
        })
        .collect()
}
