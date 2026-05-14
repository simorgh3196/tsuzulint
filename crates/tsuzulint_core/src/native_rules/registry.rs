//! Process-global registry of native rules.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::rule_trait::Rule;
use super::rules;

/// A registry of built-in, natively compiled rules keyed by name.
///
/// The registry is immutable after construction and holds `&'static` rule
/// instances, so lookups are cheap and safe to share across threads.
pub struct BuiltinRegistry {
    rules: HashMap<&'static str, &'static dyn Rule>,
}

impl BuiltinRegistry {
    /// Retrieve a rule by name, or `None` if unknown.
    pub fn get(&self, name: &str) -> Option<&'static dyn Rule> {
        self.rules.get(name).copied()
    }

    /// Names of all registered rules (useful for diagnostics / `tzlint list`).
    pub fn names(&self) -> impl Iterator<Item = &&'static str> {
        self.rules.keys()
    }

    /// Build the registry with all rules that ship in this binary.
    ///
    /// Adding a new built-in rule is a two-step change:
    /// 1. Create the rule in `native_rules::rules::...`
    /// 2. Register it here
    fn load() -> Self {
        let mut rules: HashMap<&'static str, &'static dyn Rule> = HashMap::new();
        for &rule in rules::all() {
            rules.insert(rule.name(), rule);
        }
        Self { rules }
    }
}

/// Returns a reference to the process-global built-in registry.
pub fn builtin_registry() -> &'static BuiltinRegistry {
    static REGISTRY: OnceLock<BuiltinRegistry> = OnceLock::new();
    REGISTRY.get_or_init(BuiltinRegistry::load)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_contains_no_todo() {
        let registry = builtin_registry();
        let rule = registry
            .get("no-todo")
            .expect("no-todo should be registered");
        assert_eq!(rule.name(), "no-todo");
    }

    #[test]
    fn registry_returns_none_for_unknown() {
        assert!(builtin_registry().get("made-up-rule").is_none());
    }
}
