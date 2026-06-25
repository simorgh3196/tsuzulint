//! WebAssembly bindings for TsuzuLint.
//!
//! Two artifacts, chosen by the embedder at build time:
//!
//! - **Lean** (default): the wasm-clean lint pipeline with no tokenizer backend. Tiny. Morphology-
//!   dependent rules simply stay inert. For embedders whose languages need no morphological
//!   analysis.
//! - **Full** (`--features morphology`): bundles the Japanese morphology backend, so morphology rules
//!   (e.g. `no-doubled-joshi`) fire with the same analysis as the native CLI. The embedder loads a
//!   hash-pinned, compressed dictionary from JS via `registerDictionary`.
//!
//! The pure-Rust [`Linter`] core carries the logic and is unit-tested on native; the
//! `#[wasm_bindgen]` layer is a thin wrapper compiled only for `wasm32`. The dictionary fetch and
//! its caching (IndexedDB/OPFS) live on the JS side — wasm only verifies the pin and decompresses
//! the bytes it is handed (reusing the same container/pin pipeline as the CLI).

use serde_json::Value;
use tzlint_core::{
    Config, ConfigFormat, MorphologyRegistry, ProcessorConfig, RegionRules, Registry, RuleSetting,
    lint_document,
};
use tzlint_pdk::{Diagnostic, Rule, RuleId, Severity};
use tzlint_rules::{RULE_IDS, build_rule};

/// The linter core: a resolved [`Config`] plus the built-in rule/processor registries (and, in the
/// `morphology` build, an injected morphology registry). Pure Rust — no `wasm-bindgen` — so it is
/// unit-tested on native; the `#[wasm_bindgen]` bindings below are a thin wrapper over it.
pub struct Linter {
    config: Config,
    registry: Registry,
    #[cfg(feature = "morphology")]
    morphology: MorphologyRegistry,
}

impl Linter {
    /// Build a linter from a JSON config string. An empty or whitespace-only string is the default
    /// config (every built-in rule on).
    ///
    /// # Errors
    ///
    /// The config's parse/validation error message, as a string for the JS boundary.
    pub fn from_config_json(config_json: &str) -> Result<Self, String> {
        let config = if config_json.trim().is_empty() {
            Config::default()
        } else {
            Config::parse(config_json, ConfigFormat::Json).map_err(|e| e.to_string())?
        };
        Ok(Linter {
            config,
            registry: Registry::with_builtins(),
            #[cfg(feature = "morphology")]
            morphology: MorphologyRegistry::new(),
        })
    }

    /// Lint a Markdown document, returning the diagnostics as a JSON array string.
    ///
    /// # Errors
    ///
    /// A processing-error message (e.g. an internal failure in the pipeline).
    pub fn lint_to_json(&self, text: &str) -> Result<String, String> {
        let diagnostics = self.lint(text)?;
        Ok(diagnostics_to_json(&diagnostics))
    }

    fn lint(&self, text: &str) -> Result<Vec<Diagnostic>, String> {
        let rr = RegionRules::base_only(resolve_rules(&self.config));
        let pcfg = ProcessorConfig::default();
        lint_document(
            Some("md"),
            text,
            &self.registry,
            &pcfg,
            &rr,
            self.morphology_ref(),
        )
        .map_err(|e| e.to_string())
    }

    #[cfg(feature = "morphology")]
    fn morphology_ref(&self) -> Option<&MorphologyRegistry> {
        // An empty registry behaves as `None` (no active provider → no morphology table), so this
        // is safe to pass unconditionally before any dictionary is registered.
        Some(&self.morphology)
    }

    #[cfg(not(feature = "morphology"))]
    #[allow(clippy::unused_self)]
    fn morphology_ref(&self) -> Option<&MorphologyRegistry> {
        None
    }

    /// Register a Japanese dictionary from its hash-pinned, **compressed** container bytes (handed
    /// in by the JS host, which owns fetching and caching), so morphology-dependent rules fire.
    /// Available in the `morphology` build only.
    ///
    /// The bytes are verified against `pin_hex` and decompressed in wasm (the same pipeline as the
    /// CLI) before being bridged into a provider — a wrong pin, malformed pin, or undecodable
    /// dictionary is an error, never a panic.
    ///
    /// # Errors
    ///
    /// An unsupported language, a malformed/mismatched pin, or an undecodable dictionary.
    #[cfg(feature = "morphology")]
    pub fn register_dictionary(
        &mut self,
        lang: &str,
        compressed: &[u8],
        pin_hex: &str,
    ) -> Result<(), String> {
        use tzlint_core::DictId;
        if lang != "ja" {
            return Err(format!(
                "unsupported morphology language '{lang}' (only 'ja')"
            ));
        }
        let pin = decode_pin(pin_hex)?;
        let bytes =
            tzlint_core::decompress_dictionary(compressed, &pin).map_err(|e| e.to_string())?;
        let provider = tzlint_morphology_native::LinderaProvider::from_dictionary_bytes(&bytes)
            .map_err(|e| e.to_string())?;
        self.morphology
            .insert(Box::new(provider), DictId::from_pin(pin));
        Ok(())
    }
}

/// Resolve the active built-in rule set for `config`: every rule on by default, minus those a
/// `config.rules` entry turns off, each with its configured options/severity. (Column overlays —
/// a CLI/CSV concern — are not modeled here; the browser lints Markdown/text.)
fn resolve_rules(config: &Config) -> Vec<Box<dyn Rule>> {
    RULE_IDS
        .iter()
        .filter_map(|id| match config.rules.get(&RuleId::from(*id)) {
            Some(RuleSetting::Off) => None,
            Some(RuleSetting::On { severity, options }) => build_rule(id, options, *severity),
            None => build_rule(id, &Value::Null, None),
        })
        .collect()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticJson<'a> {
    rule_id: &'a str,
    severity: &'static str,
    message: &'a str,
    start: u32,
    end: u32,
}

/// Serialize diagnostics to a compact JSON array. Spans are byte offsets into the source.
fn diagnostics_to_json(diagnostics: &[Diagnostic]) -> String {
    let items: Vec<DiagnosticJson> = diagnostics
        .iter()
        .map(|d| DiagnosticJson {
            rule_id: d.rule_id.as_str(),
            severity: severity_str(d.severity),
            message: &d.message,
            start: d.span.start,
            end: d.span.end,
        })
        .collect();
    serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
}

/// The lowercase wire spelling of a severity (matches the config schema's `severity` enum).
fn severity_str(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
    }
}

/// Decode 64 hex characters into a 32-byte pin, panic-free.
#[cfg(feature = "morphology")]
fn decode_pin(hex: &str) -> Result<[u8; 32], String> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return Err("dictionary pin must be 64 hexadecimal characters".to_string());
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        let hi = hex_nibble(bytes[i * 2]).ok_or("dictionary pin has a non-hex character")?;
        let lo = hex_nibble(bytes[i * 2 + 1]).ok_or("dictionary pin has a non-hex character")?;
        *slot = (hi << 4) | lo;
    }
    Ok(out)
}

/// The value of one hex digit (`0-9`, `a-f`, `A-F`), or `None`.
#[cfg(feature = "morphology")]
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// The `#[wasm_bindgen]` JS bindings — a thin wrapper over [`Linter`], compiled only for `wasm32`.
#[cfg(target_arch = "wasm32")]
mod bindings {
    use wasm_bindgen::prelude::*;

    use super::Linter;

    /// The JS-facing linter handle.
    #[wasm_bindgen]
    pub struct TsuzuLint {
        inner: Linter,
    }

    #[wasm_bindgen]
    impl TsuzuLint {
        /// Construct a linter from a JSON config string (empty ⇒ the default config).
        #[wasm_bindgen(constructor)]
        pub fn new(config_json: &str) -> Result<TsuzuLint, JsError> {
            console_error_panic_hook::set_once();
            let inner = Linter::from_config_json(config_json).map_err(|e| JsError::new(&e))?;
            Ok(TsuzuLint { inner })
        }

        /// Lint a Markdown document; returns a JSON array of diagnostics
        /// (`[{ ruleId, severity, message, start, end }]`).
        pub fn lint(&self, text: &str) -> Result<String, JsError> {
            self.inner.lint_to_json(text).map_err(|e| JsError::new(&e))
        }

        /// Register a hash-pinned, compressed Japanese dictionary so morphology rules fire (the
        /// `morphology` build only). `compressed` is the `.dict.zst` bytes; `pin_hex` is the 64-char
        /// BLAKE3 pin over them.
        #[cfg(feature = "morphology")]
        #[wasm_bindgen(js_name = registerDictionary)]
        pub fn register_dictionary(
            &mut self,
            lang: &str,
            compressed: &[u8],
            pin_hex: &str,
        ) -> Result<(), JsError> {
            self.inner
                .register_dictionary(lang, compressed, pin_hex)
                .map_err(|e| JsError::new(&e))
        }
    }

    // Run in a real wasm runtime (node) via `wasm-bindgen-test-runner` — the only place the JS-facing
    // `TsuzuLint` is executed rather than just type-checked, so the `#[wasm_bindgen]` string/byte
    // marshaling and the `JsError` mapping are exercised (the native `Linter` tests below never enter
    // this `cfg(target_arch = "wasm32")` module).
    #[cfg(test)]
    mod tests {
        use wasm_bindgen_test::wasm_bindgen_test;

        use super::TsuzuLint;

        #[wasm_bindgen_test]
        fn new_then_lint_returns_diagnostics_json() {
            let linter = TsuzuLint::new("").ok().unwrap();
            // Half-width kana trips `no-hankaku-kana` under the default (all-on) rule set.
            let json = linter.lint("ﾊﾛｰ\n").ok().unwrap();
            assert!(json.contains("no-hankaku-kana"), "{json}");
            assert!(json.contains("\"start\""), "{json}");
        }

        #[wasm_bindgen_test]
        fn a_disabled_rule_does_not_fire() {
            let linter = TsuzuLint::new(r#"{ "rules": { "no-hankaku-kana": false } }"#)
                .ok()
                .unwrap();
            assert_eq!(linter.lint("ﾊﾛｰ\n").ok().unwrap(), "[]");
        }

        #[wasm_bindgen_test]
        fn an_invalid_config_surfaces_an_error() {
            assert!(TsuzuLint::new("{ not json").is_err());
        }

        // With the morphology backend bundled in, the dictionary-registration boundary must surface
        // bad input as a JS error through the wasm marshaling, never panic.
        #[cfg(feature = "morphology")]
        #[wasm_bindgen_test]
        fn register_dictionary_surfaces_errors_without_panicking() {
            let mut linter = TsuzuLint::new("").ok().unwrap();
            assert!(
                linter
                    .register_dictionary("ko", b"x", &"0".repeat(64))
                    .is_err()
            );
            assert!(linter.register_dictionary("ja", b"x", "abc").is_err());
            assert!(
                linter
                    .register_dictionary("ja", b"not a real dictionary", &"0".repeat(64))
                    .is_err()
            );
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn lints_markdown_with_the_default_rule_set() {
        let linter = Linter::from_config_json("").unwrap();
        // Half-width kana triggers `no-hankaku-kana` under the default (all-on) set.
        let json = linter.lint_to_json("ﾊﾛｰ\n").unwrap();
        assert!(json.contains("no-hankaku-kana"), "{json}");
        assert!(json.contains("\"severity\""), "{json}");
        assert!(json.contains("\"start\""), "{json}");
    }

    #[test]
    fn a_blank_config_is_the_default() {
        assert!(Linter::from_config_json("   ").is_ok());
    }

    #[test]
    fn a_rule_disabled_in_config_does_not_fire() {
        let linter =
            Linter::from_config_json(r#"{ "rules": { "no-hankaku-kana": false } }"#).unwrap();
        let json = linter.lint_to_json("ﾊﾛｰ\n").unwrap();
        assert_eq!(json, "[]", "disabled rule must not fire: {json}");
    }

    #[test]
    fn an_invalid_config_is_an_error() {
        assert!(Linter::from_config_json("{ not json").is_err());
    }

    #[test]
    fn clean_text_yields_an_empty_array() {
        let linter = Linter::from_config_json("").unwrap();
        assert_eq!(linter.lint_to_json("ただのテキストです。\n").unwrap(), "[]");
    }

    #[test]
    fn a_rule_with_options_and_a_severity_override_is_applied() {
        // The object rule form exercises the `RuleSetting::On { options, severity }` path: max-ten
        // with `max: 0` flags any comma, and the severity override surfaces as "error" in the JSON.
        let linter = Linter::from_config_json(
            r#"{ "rules": { "max-ten": { "severity": "error", "options": { "max": 0 } } } }"#,
        )
        .unwrap();
        let json = linter.lint_to_json("あれ、これ、それ。\n").unwrap();
        assert!(json.contains("max-ten"), "{json}");
        assert!(json.contains("\"severity\":\"error\""), "{json}");
    }

    #[test]
    fn severity_renders_every_variant() {
        assert_eq!(severity_str(Severity::Error), "error");
        assert_eq!(severity_str(Severity::Warning), "warning");
        assert_eq!(severity_str(Severity::Info), "info");
        assert_eq!(severity_str(Severity::Hint), "hint");
    }

    #[cfg(feature = "morphology")]
    #[test]
    fn register_dictionary_rejects_bad_lang_and_pin() {
        let mut linter = Linter::from_config_json("").unwrap();
        // Unsupported language (rejected before the pin is even decoded).
        assert!(
            linter
                .register_dictionary("ko", b"x", &"0".repeat(64))
                .is_err()
        );
        // Malformed pin (wrong length).
        assert!(linter.register_dictionary("ja", b"x", "abc").is_err());
        // 64-char but non-hex pin.
        let bad = format!("g{}", "0".repeat(63));
        assert!(linter.register_dictionary("ja", b"x", &bad).is_err());
        // A WELL-FORMED pin but bytes that do not hash to it: the pin decodes, then the
        // verify+decompress rejects the blob (a hash mismatch) — exercising the success path of
        // pin decoding and the decompress call, without needing a real dictionary.
        let err = linter
            .register_dictionary("ja", b"not a real dictionary", &"0".repeat(64))
            .unwrap_err();
        assert!(!err.is_empty(), "a mismatched blob must surface an error");
    }
}
