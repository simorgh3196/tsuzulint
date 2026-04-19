//! Preset bundles of native rules.
//!
//! A preset enables a curated set of rules with sensible defaults so users
//! don't have to wire each rule individually. Modeled after textlint's
//! `preset-*` packages for familiarity.

use serde_json::Value;

/// Entry in a preset: rule name + default options (merged with user options
/// from `.tsuzulint.jsonc` at lint time).
#[derive(Debug, Clone)]
pub struct PresetEntry {
    pub name: &'static str,
    pub default_options: Value,
}

impl PresetEntry {
    const fn bare(name: &'static str) -> Self {
        Self {
            name,
            default_options: Value::Null,
        }
    }
}

/// Resolve a preset name to its list of enabled rules, or `None` for an
/// unknown preset.
pub fn resolve_preset(name: &str) -> Option<Vec<PresetEntry>> {
    match name {
        "ja-technical-writing" | "preset-ja-technical-writing" => Some(ja_technical_writing()),
        "ja-basic" | "preset-ja-basic" => Some(ja_basic()),
        _ => None,
    }
}

/// `ja-technical-writing`: the big bundle that matches textlint's
/// technical-writing preset as closely as we currently can.
fn ja_technical_writing() -> Vec<PresetEntry> {
    vec![
        PresetEntry {
            name: "sentence-length",
            default_options: serde_json::json!({ "max": 100 }),
        },
        PresetEntry {
            name: "max-ten",
            default_options: serde_json::json!({ "max": 3 }),
        },
        PresetEntry {
            name: "max-kanji-continuous-len",
            default_options: serde_json::json!({ "max": 6 }),
        },
        PresetEntry::bare("no-doubled-joshi"),
        PresetEntry::bare("no-mixed-zenkaku-hankaku-alphabet"),
        PresetEntry::bare("no-exclamation-question-mark"),
        PresetEntry::bare("no-hankaku-kana"),
        PresetEntry::bare("no-nfd"),
        PresetEntry::bare("no-zero-width-spaces"),
        PresetEntry::bare("ja-no-mixed-period"),
    ]
}

/// `ja-basic`: a lighter bundle aimed at casual blogs and docs — no length
/// limits, just the text-quality checks.
fn ja_basic() -> Vec<PresetEntry> {
    vec![
        PresetEntry::bare("no-mixed-zenkaku-hankaku-alphabet"),
        PresetEntry::bare("no-hankaku-kana"),
        PresetEntry::bare("no-nfd"),
        PresetEntry::bare("no-zero-width-spaces"),
        PresetEntry::bare("ja-no-mixed-period"),
    ]
}

/// Names of all presets this binary ships.
pub fn all_preset_names() -> &'static [&'static str] {
    &["ja-technical-writing", "ja-basic"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_ja_technical_writing() {
        let preset = resolve_preset("ja-technical-writing").unwrap();
        assert!(preset.iter().any(|e| e.name == "sentence-length"));
        assert!(preset.iter().any(|e| e.name == "max-ten"));
    }

    #[test]
    fn resolves_prefixed_name() {
        assert!(resolve_preset("preset-ja-technical-writing").is_some());
    }

    #[test]
    fn unknown_preset_returns_none() {
        assert!(resolve_preset("nope").is_none());
    }

    #[test]
    fn all_presets_resolve() {
        for name in all_preset_names() {
            assert!(
                resolve_preset(name).is_some(),
                "preset '{}' should resolve",
                name
            );
        }
    }
}
