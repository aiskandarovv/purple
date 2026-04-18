//! Containers overlay state.

/// State for the Containers overlay.
///
/// No `Default` impl: construction always requires an alias and runtime
/// metadata, so a default-constructed ContainerState would be meaningless.
/// Call sites always pass Some(ContainerState { alias, ... }) into App.
pub struct ContainerState {
    pub alias: String,
    pub askpass: Option<String>,
    pub runtime: Option<crate::containers::ContainerRuntime>,
    pub containers: Vec<crate::containers::ContainerInfo>,
    pub list_state: ratatui::widgets::ListState,
    pub loading: bool,
    pub error: Option<String>,
    pub action_in_progress: Option<String>,
    /// Pending confirmation for stop/restart actions: (action, container_name, container_id).
    pub confirm_action: Option<(crate::containers::ContainerAction, String, String)>,
}
