//! Resolving a [`Config`] to the set of [`Rule`]s the engine should run.
//!
//! This is the one part of the pipeline that is *not* wired yet. The built-in rule registry
//! lives in `tzlint_rules`, which is still a skeleton on `main` (its `Rule` impls and the
//! `builtin_rules()` constructor land with the rules milestone). Until then there are no
//! native rules to construct, so this returns an empty set: the rest of the pipeline
//! (read → parse → archive → engine → output, plus config discovery and the cache) is fully
//! wired and exercised, and the engine simply reports nothing.
//!
//! When the registry lands, this function maps `config.rules` (and any preset) onto the
//! built-in rules — turning settings on/off, applying severity overrides, and threading each
//! rule's `options` through. That is a localized change to this single file.

use tzlint_core::Config;
use tzlint_pdk::Rule;

/// Build the boxed rule set to run for `config`.
///
/// Currently always empty — see the module docs. `config` is accepted now so the signature
/// is stable across the registry-wiring change.
#[must_use]
pub fn resolve_rules(_config: &Config) -> Vec<Box<dyn Rule>> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_rules_are_wired_yet() {
        // A guard on the documented current state: the registry is not wired, so any config
        // resolves to zero rules. Update this when `tzlint_rules` lands its registry.
        assert!(resolve_rules(&Config::default()).is_empty());
    }
}
