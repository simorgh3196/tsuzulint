//! Parallel file linting logic.

use rayon::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::warn;
use tsuzulint_cache::CacheManager;
use tsuzulint_text::Tokenizer;

use crate::config::LinterConfig;
use crate::error::LinterError;
use crate::file_linter::lint_file_internal;
use crate::result::LintResult;
use crate::rule_loader::create_plugin_host;

pub type LintFilesResult = Result<(Vec<LintResult>, Vec<(PathBuf, LinterError)>), LinterError>;

pub fn lint_files(
    paths: &[PathBuf],
    config: &LinterConfig,
    config_hash: &[u8; 32],
    tokenizer: &Arc<Tokenizer>,
    cache: &std::sync::Mutex<CacheManager>,
    dynamic_rules: &std::sync::Mutex<Vec<PathBuf>>,
) -> LintFilesResult {
    let enabled_rules_vec = config.enabled_rules();
    let enabled_rules: std::collections::HashSet<&str> =
        enabled_rules_vec.iter().map(|(n, _)| *n).collect();
    let timings_enabled = config.timings;

    let results: Vec<Result<LintResult, (PathBuf, LinterError)>> = paths
        .par_iter()
        .map_init(
            || {
                let host_result = create_plugin_host(config, dynamic_rules);
                host_result.map(|host| {
                    // Optimization: We compute `rule_versions` once per parallel thread (initialization phase)
                    // and pass it down. This completely eliminates the per-file HashMap allocation
                    // during `lint_file_internal` which significantly speeds up large codebase linting.
                    let rule_versions = crate::rule_loader::get_rule_versions_from_host(&host);
                    (host, rule_versions)
                })
            },
            |host_result, path| {
                let (file_host, rule_versions) = match host_result.as_mut() {
                    Ok(h) => h,
                    Err(e) => {
                        return Err((
                            path.clone(),
                            LinterError::Internal(format!(
                                "Failed to initialize plugin host: {}",
                                e
                            )),
                        ));
                    }
                };

                let mut ctx = crate::file_linter::LintContext {
                    path,
                    host: file_host,
                    tokenizer,
                    config_hash,
                    cache,
                    enabled_rules: &enabled_rules,
                    rule_versions,
                    timings_enabled,
                };

                lint_file_internal(&mut ctx).map_err(|e| (path.clone(), e))
            },
        )
        .collect();

    let mut successes = Vec::new();
    let mut failures = Vec::new();
    for result in results {
        match result {
            Ok(lint_result) => successes.push(lint_result),
            Err((path, error)) => {
                warn!("Failed to lint {}: {}", path.display(), error);
                failures.push((path, error));
            }
        }
    }

    match cache.lock() {
        Ok(cache) => {
            if let Err(e) = cache.save() {
                warn!("Failed to save cache: {}", e);
            }
        }
        Err(poison) => {
            warn!("Cache mutex poisoned, attempting recovery: {}", poison);
            if let Err(e) = poison.into_inner().save() {
                warn!("Failed to save cache after recovery: {}", e);
            }
        }
    }

    Ok((successes, failures))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lint_files_parallel_empty() {
        let config = LinterConfig::new();
        let config_hash = config.hash().unwrap();
        let tokenizer = Arc::new(Tokenizer::new().unwrap());
        let cache_dir = std::env::temp_dir().join("tsuzulint_test_empty");
        let _ = std::fs::create_dir_all(&cache_dir);
        let cache = std::sync::Mutex::new(CacheManager::new(cache_dir));
        let dynamic_rules = std::sync::Mutex::new(Vec::new());

        let paths: Vec<PathBuf> = vec![];
        let result = lint_files(
            &paths,
            &config,
            &config_hash,
            &tokenizer,
            &cache,
            &dynamic_rules,
        );
        assert!(result.is_ok());

        let (successes, failures) = result.unwrap();
        assert!(successes.is_empty());
        assert!(failures.is_empty());
    }
}
