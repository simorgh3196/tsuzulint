//! Configuration management for LSP server.

use tracing::{error, info};

use tsuzulint_core::{Linter, LinterConfig};

use crate::state::BackendState;

/// Reloads configuration from the workspace root.
pub fn reload_config(state: &BackendState) {
    let root_guard = match state.workspace_root.read() {
        Ok(g) => g,
        Err(e) => {
            error!("Workspace root lock poisoned: {}", e);
            return;
        }
    };

    let path = match root_guard.as_ref() {
        Some(p) => p,
        None => return,
    };

    if let Some(config_path) = LinterConfig::discover(path) {
        info!("Found config file: {}", config_path.display());
        match LinterConfig::from_file(&config_path) {
            Ok(config) => {
                info!("Loaded configuration from workspace");
                match state.linter.write() {
                    Ok(mut linter_guard) => match Linter::new(config) {
                        Ok(new_linter) => {
                            *linter_guard = Some(new_linter);
                            info!("Linter re-initialized with new config");
                        }
                        Err(e) => {
                            error!("Failed to create new linter: {}", e);
                            *linter_guard = None;
                        }
                    },
                    Err(e) => error!("Linter lock poisoned: {}", e),
                }
            }
            Err(e) => {
                error!("Failed to load config: {}", e);
            }
        }
    } else {
        info!("No config file found; clearing linter");
        match state.linter.write() {
            Ok(mut g) => *g = None,
            Err(e) => error!("Linter lock poisoned: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn setup_state_with_config(temp: &tempfile::TempDir) -> BackendState {
        let config_path = temp.path().join(".tsuzulint.json");
        fs::write(&config_path, "{}").unwrap();
        let state = BackendState::new();
        {
            let mut root = state.workspace_root.write().unwrap();
            *root = Some(temp.path().to_path_buf());
        }
        state
    }

    #[test]
    fn test_reload_config_clears_linter_when_config_deleted() {
        let temp = tempdir().unwrap();
        let state = setup_state_with_config(&temp);

        reload_config(&state);
        assert!(state.linter.read().unwrap().is_some());

        fs::remove_file(temp.path().join(".tsuzulint.json")).unwrap();

        reload_config(&state);

        assert!(
            state.linter.read().unwrap().is_none(),
            "Linter should be cleared when config file is deleted"
        );
    }

    #[test]
    fn test_reload_config_loads_linter_when_config_exists() {
        let temp = tempdir().unwrap();
        let state = setup_state_with_config(&temp);

        assert!(state.linter.read().unwrap().is_none());

        reload_config(&state);

        assert!(state.linter.read().unwrap().is_some());
    }

    #[test]
    fn test_reload_config_no_op_when_no_workspace_root() {
        let state = BackendState::new();

        reload_config(&state);

        assert!(state.linter.read().unwrap().is_none());
    }
}
