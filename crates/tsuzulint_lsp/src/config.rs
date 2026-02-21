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
    }
}
