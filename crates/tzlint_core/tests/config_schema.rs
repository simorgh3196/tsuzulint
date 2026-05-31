//! Differential fidelity test: the published [`CONFIG_SCHEMA`] vs the real config loader.
//!
//! This proves the schema is a faithful **JSON-level** contract for `Config::parse(_,
//! ConfigFormat::Json)`. Each corpus fixture is tagged with how the schema and loader should
//! relate:
//!
//! - `Accept` — both the loader and the schema accept it (strict parity).
//! - `Reject` — both reject it (strict parity).
//! - `AsymStrict` — the loader accepts it but the schema rejects it. This is the single
//!   deliberate divergence: the loader accepts string boolean spellings (`"on"`/`"yes"`/`"true"`
//!   and `false` counterparts, case-insensitively) via `deserialize_any`/`visit_str`, but the
//!   published schema accepts only real booleans, to steer authors toward canonical
//!   `true`/`false`.
//!
//! Each fixture's tag is also asserted against the loader's *actual* result, so the corpus
//! cannot silently rot, and every `Accept`/`Reject` fixture asserts schema⇔loader parity, so a
//! new *accidental* divergence (outside the `AsymStrict` set) fails this test.
//!
//! Out of scope (covered by unit tests in `src/config/`, not here, and intentionally absent
//! from the corpus): the YAML-only leniency (yes/no/on/off booleans, BOM, JSONC comments,
//! anchor rejection), serde's duplicate-key rejection (not expressible in JSON Schema), and the
//! empty/whitespace/comments-only document mapping to `Config::default()` before deserialization
//! (an empty string is not valid JSON, so it is outside the schema's domain).

// This is an integration test (its own crate); `unwrap`/`expect` are the right way to fail
// loudly on a broken embedded schema. `allow-*-in-tests` only covers `#[test]` fns, not the
// `validator()` helper, so allow them file-wide here.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;

use jsonschema::Validator;
use serde_json::Value;
use tzlint_core::{CONFIG_SCHEMA, Config, ConfigError, ConfigFormat};

/// The lowercase severity spellings the loader accepts (`SeverityRepr`, `rename_all =
/// "lowercase"`). Adding a `Severity` variant to the PDK means updating `SeverityRepr`, the
/// schema's `severity` enum, AND this list — `schema_severity_enum_matches_loader` enforces it.
const SEVERITIES: &[&str] = &["error", "warning", "info", "hint"];

#[derive(Clone, Copy, Debug)]
enum Mode {
    /// Loader accepts and schema accepts.
    Accept,
    /// Loader rejects and schema rejects.
    Reject,
    /// Loader accepts but schema deliberately rejects (string-boolean leniency).
    AsymStrict,
}

const CORPUS: &[(&str, &str, Mode)] = &[
    // Top-level shape.
    ("empty object", "{}", Mode::Accept),
    ("only language", r#"{"language":"ja"}"#, Mode::Accept),
    ("language null", r#"{"language":null}"#, Mode::Accept),
    ("language wrong type", r#"{"language":42}"#, Mode::Reject),
    (
        "message-language",
        r#"{"message-language":"en"}"#,
        Mode::Accept,
    ),
    (
        "unknown top-level key",
        r#"{"langauge":"ja"}"#,
        Mode::Reject,
    ),
    // `extends` is reserved: only null (= absent) is accepted; any non-null value is rejected.
    ("extends null", r#"{"extends":null}"#, Mode::Accept),
    ("extends array", r#"{"extends":["ja-basic"]}"#, Mode::Reject),
    ("extends number", r#"{"extends":42}"#, Mode::Reject),
    // Rules map.
    ("rules empty", r#"{"rules":{}}"#, Mode::Accept),
    ("rule false", r#"{"rules":{"r":false}}"#, Mode::Accept),
    ("rule true", r#"{"rules":{"r":true}}"#, Mode::Accept),
    ("rule empty object", r#"{"rules":{"r":{}}}"#, Mode::Accept),
    (
        "rule severity error",
        r#"{"rules":{"r":{"severity":"error"}}}"#,
        Mode::Accept,
    ),
    (
        "rule severity warning",
        r#"{"rules":{"r":{"severity":"warning"}}}"#,
        Mode::Accept,
    ),
    (
        "rule severity info",
        r#"{"rules":{"r":{"severity":"info"}}}"#,
        Mode::Accept,
    ),
    (
        "rule severity hint",
        r#"{"rules":{"r":{"severity":"hint"}}}"#,
        Mode::Accept,
    ),
    (
        "rule severity bad",
        r#"{"rules":{"r":{"severity":"fatal"}}}"#,
        Mode::Reject,
    ),
    // `options` is any JSON value — every type must validate.
    (
        "rule options null",
        r#"{"rules":{"r":{"options":null}}}"#,
        Mode::Accept,
    ),
    (
        "rule options number",
        r#"{"rules":{"r":{"options":3}}}"#,
        Mode::Accept,
    ),
    (
        "rule options string",
        r#"{"rules":{"r":{"options":"x"}}}"#,
        Mode::Accept,
    ),
    (
        "rule options array",
        r#"{"rules":{"r":{"options":[1,2]}}}"#,
        Mode::Accept,
    ),
    (
        "rule options object",
        r#"{"rules":{"r":{"options":{"max":3}}}}"#,
        Mode::Accept,
    ),
    (
        "rule options bool",
        r#"{"rules":{"r":{"options":true}}}"#,
        Mode::Accept,
    ),
    (
        "rule severity + options",
        r#"{"rules":{"r":{"severity":"error","options":{"max":3}}}}"#,
        Mode::Accept,
    ),
    (
        "rule unknown key",
        r#"{"rules":{"r":{"severty":"error"}}}"#,
        Mode::Reject,
    ),
    ("rule value number", r#"{"rules":{"r":5}}"#, Mode::Reject),
    ("rule value array", r#"{"rules":{"r":[1]}}"#, Mode::Reject),
    ("rule value null", r#"{"rules":{"r":null}}"#, Mode::Reject),
    ("rules non-object", r#"{"rules":5}"#, Mode::Reject),
    // AsymStrict: the loader accepts string booleans (visit_str); the schema rejects them.
    (
        "rule string on",
        r#"{"rules":{"r":"on"}}"#,
        Mode::AsymStrict,
    ),
    (
        "rule string yes",
        r#"{"rules":{"r":"yes"}}"#,
        Mode::AsymStrict,
    ),
    (
        "rule string true",
        r#"{"rules":{"r":"true"}}"#,
        Mode::AsymStrict,
    ),
    (
        "rule string off",
        r#"{"rules":{"r":"off"}}"#,
        Mode::AsymStrict,
    ),
    (
        "rule string no",
        r#"{"rules":{"r":"no"}}"#,
        Mode::AsymStrict,
    ),
    (
        "rule string false",
        r#"{"rules":{"r":"false"}}"#,
        Mode::AsymStrict,
    ),
    (
        "rule string uppercase",
        r#"{"rules":{"r":"ON"}}"#,
        Mode::AsymStrict,
    ),
    // A non-boolean string is rejected by BOTH, so it is plain Reject (not AsymStrict).
    (
        "rule string invalid",
        r#"{"rules":{"r":"maybe"}}"#,
        Mode::Reject,
    ),
];

fn validator() -> Validator {
    let schema: Value = serde_json::from_str(CONFIG_SCHEMA).expect("CONFIG_SCHEMA is valid JSON");
    jsonschema::validator_for(&schema).expect("CONFIG_SCHEMA builds into a validator")
}

#[test]
fn schema_matches_loader_with_documented_asymmetry() {
    let validator = validator();
    for (name, text, mode) in CORPUS {
        let value: Value = serde_json::from_str(text)
            .unwrap_or_else(|e| panic!("{name}: corpus text not JSON: {e}"));
        let schema_ok = validator.is_valid(&value);
        let loader_ok = Config::parse(text, ConfigFormat::Json).is_ok();
        match mode {
            Mode::Accept => {
                assert!(loader_ok, "{name}: loader should accept (tag rotted?)");
                assert!(schema_ok, "{name}: schema should accept but rejected");
            }
            Mode::Reject => {
                assert!(!loader_ok, "{name}: loader should reject (tag rotted?)");
                assert!(!schema_ok, "{name}: schema should reject but accepted");
            }
            Mode::AsymStrict => {
                assert!(
                    loader_ok,
                    "{name}: loader should accept (string-bool leniency)"
                );
                assert!(
                    !schema_ok,
                    "{name}: schema is intentionally strict and should reject string booleans"
                );
            }
        }
    }
}

#[test]
fn extends_rejection_is_reserved_not_parse() {
    // The schema can only say accept/reject; the loader distinguishes a *reserved* key from a
    // generic parse error. Pin that boundary so a refactor can't silently downgrade it.
    for text in [r#"{"extends":["ja-basic"]}"#, r#"{"extends":42}"#] {
        assert!(
            matches!(
                Config::parse(text, ConfigFormat::Json),
                Err(ConfigError::Reserved("extends"))
            ),
            "{text} should be a reserved-key error"
        );
    }
    // A generic bad value is a parse error, not reserved.
    assert!(matches!(
        Config::parse(r#"{"language":42}"#, ConfigFormat::Json),
        Err(ConfigError::Parse { .. })
    ));
    // `extends: null` is accepted as a no-op.
    assert!(Config::parse(r#"{"extends":null}"#, ConfigFormat::Json).is_ok());
}

#[test]
fn schema_severity_enum_matches_loader() {
    let schema: Value = serde_json::from_str(CONFIG_SCHEMA).expect("valid JSON");
    let enum_values = schema["$defs"]["severity"]["enum"]
        .as_array()
        .expect("$defs.severity.enum is an array");
    let in_schema: BTreeSet<&str> = enum_values
        .iter()
        .map(|v| v.as_str().expect("severity enum entries are strings"))
        .collect();
    let expected: BTreeSet<&str> = SEVERITIES.iter().copied().collect();
    assert_eq!(
        in_schema, expected,
        "schema severity enum must equal the loader's SeverityRepr spellings exactly"
    );
    // And the loader must actually accept each (and reject a non-member), keeping both sides honest.
    for s in SEVERITIES {
        let text = format!(r#"{{"rules":{{"r":{{"severity":"{s}"}}}}}}"#);
        assert!(
            Config::parse(&text, ConfigFormat::Json).is_ok(),
            "loader should accept severity {s}"
        );
    }
    assert!(
        Config::parse(
            r#"{"rules":{"r":{"severity":"fatal"}}}"#,
            ConfigFormat::Json
        )
        .is_err()
    );
}

#[test]
fn embedded_schema_is_valid_and_self_describing() {
    let schema: Value =
        serde_json::from_str(CONFIG_SCHEMA).expect("CONFIG_SCHEMA must be valid JSON");
    assert!(
        jsonschema::meta::is_valid(&schema),
        "CONFIG_SCHEMA must be a valid Draft 2020-12 schema"
    );
    assert_eq!(
        schema["$schema"],
        Value::from("https://json-schema.org/draft/2020-12/schema"),
        "must declare the Draft 2020-12 metaschema"
    );
    assert_eq!(
        schema["$id"],
        Value::from("https://tsuzulint.dev/schema/config/v1.json"),
        "must carry a stable versioned $id"
    );
}

/// Valid top-level contexts to wrap a single rule in; none independently causes rejection, so
/// each combined document's outcome is governed solely by the rule value.
const VALID_WRAPPERS: &[&str] = &[
    "",
    r#""language":"ja","#,
    r#""message-language":"en","#,
    r#""language":"ja","message-language":"en","#,
];

/// Rule-value fragments crossed with the wrappers, with how schema vs loader should relate.
const RULE_VALUES: &[(&str, Mode)] = &[
    ("false", Mode::Accept),
    ("true", Mode::Accept),
    ("{}", Mode::Accept),
    (r#"{"severity":"warning"}"#, Mode::Accept),
    (r#"{"options":[1,{"a":2}]}"#, Mode::Accept),
    (r#"{"severity":"hint","options":null}"#, Mode::Accept),
    (r#"{"severity":"nope"}"#, Mode::Reject),
    (r#"{"bogus":1}"#, Mode::Reject),
    ("123", Mode::Reject),
    ("null", Mode::Reject),
    ("[true]", Mode::Reject),
    (r#""on""#, Mode::AsymStrict),
    (r#""OFF""#, Mode::AsymStrict),
    (r#""maybe""#, Mode::Reject),
];

/// Broaden coverage beyond the hand corpus: cross every valid top-level wrapper with every rule
/// value and assert the schema/loader invariant on the combined document — the only permitted
/// divergence is the documented `AsymStrict` string-boolean case.
#[test]
fn schema_loader_invariant_over_wrapper_and_rule_cross_product() {
    let validator = validator();
    for wrapper in VALID_WRAPPERS {
        for (frag, mode) in RULE_VALUES {
            let text = format!(r#"{{{wrapper}"rules":{{"r":{frag}}}}}"#);
            let value: Value = serde_json::from_str(&text).expect("generated doc is JSON");
            let schema_ok = validator.is_valid(&value);
            let loader_ok = Config::parse(&text, ConfigFormat::Json).is_ok();
            match mode {
                Mode::Accept => assert!(loader_ok && schema_ok, "{text}: expected both to accept"),
                Mode::Reject => {
                    assert!(!loader_ok && !schema_ok, "{text}: expected both to reject")
                }
                Mode::AsymStrict => assert!(
                    loader_ok && !schema_ok,
                    "{text}: expected loader-accept, schema-reject"
                ),
            }
        }
    }
}
