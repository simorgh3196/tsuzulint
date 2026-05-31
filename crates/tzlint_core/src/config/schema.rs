//! The published JSON Schema for TsuzuLint config files.

/// The canonical JSON Schema (Draft 2020-12) for `.tzlintrc*` files, embedded verbatim.
///
/// This is the single source of truth for the config's **JSON-level** contract: editors can
/// bind it to `.tzlintrc*` for completion/validation, and the CLI emits it (M1g). It mirrors
/// the loader's `deny_unknown_fields` strictness and the `false | true | { severity?, options? }`
/// rule-setting shape.
///
/// It is intentionally **stricter than the loader on one point**: the loader also accepts the
/// string boolean spellings YAML produces (`"true"`/`"yes"`/`"on"`/… and their `false`
/// counterparts, case-insensitively) because [`RuleSetting`](crate::RuleSetting) deserializes
/// via `deserialize_any`, whereas the schema accepts only real JSON booleans — to steer authors
/// toward canonical `true`/`false`. `tests/config_schema.rs` pins both the agreement and that
/// single deliberate asymmetry against the real loader, so the two cannot silently drift.
pub const CONFIG_SCHEMA: &str = include_str!("config.schema.json");
