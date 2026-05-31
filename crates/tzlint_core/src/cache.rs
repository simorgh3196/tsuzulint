//! Document-level cache: cache a document's lint result keyed by *everything* that can change
//! it, so re-linting an unchanged document with unchanged config is a hit.
//!
//! [`document_cache_key`] folds the document content, the full [`Config`], the
//! parser/engine/transform versions, the (M1f) rule versions, and a (M2) morphology
//! fingerprint into a single 32-byte BLAKE3 [`CacheKey`]. [`DocumentCache`] is a bounded,
//! process-local, in-memory map from that key to the final `Vec<Diagnostic>`; [`lint_cached`]
//! is the convenience that wires `parse → archive → Engine::lint` on a miss.
//!
//! **Correctness rule:** the cache is a pure function of its key, so *every* input that can
//! change a diagnostic MUST be in the key. Before merging any change that makes diagnostics
//! depend on a new input, add that input to [`document_cache_key`]. BLAKE3 is used (not a fast
//! non-cryptographic hash) precisely because a key collision would return another document's
//! diagnostics — its 256-bit collision resistance makes that infeasible.
//!
//! **Versioning:** the parser/transform/engine versions below are hand-bumped consts, *not*
//! `CARGO_PKG_VERSION` (the workspace is pinned `0.0.0`, so it carries no signal). Bump the
//! relevant const whenever the corresponding behavior changes (see each const's doc).
//!
//! **Scope:** only the in-memory cache exists; persistence is deferred to the CLI (M1g),
//! where a `Cache` trait would gain a native-only `Host`-backed implementation keyed by
//! [`CacheKey::to_hex`], stamping the version blob per entry and discarding on mismatch so an
//! upgraded binary can never serve a stale on-disk hit. Keeping the cache in-memory-only means a
//! stale result cannot outlive the process.

use std::collections::{HashMap, VecDeque};
use std::fmt;

use blake3::Hasher;
use tzlint_pdk::{Diagnostic, Rule, Severity};

use crate::config::{Config, RuleSetting};
use crate::engine::Engine;
use crate::parse::{ParseError, parse};

/// Cache-key recipe version. Bump to invalidate every key if the key *construction* changes.
const KEY_SCHEMA_VERSION: u32 = 1;
/// The markdown parser pin. Bump when the `markdown` dependency's major/minor changes.
const PARSER_VERSION: &str = "markdown@1.0";
/// The mdast → index-AST transform and enabled constructs. Bump on any change to the transform,
/// the `NodeKind` mapping, or `parse_options` (GFM/frontmatter set).
const AST_TRANSFORM_VERSION: &str = "v1";
/// Engine traversal/dispatch and the diagnostic sort order. Bump on any change to
/// `Engine::lint`'s walk or its `(span.start, span.end, rule_id, message)` ordering.
const ENGINE_BEHAVIOR_VERSION: &str = "v1";
/// The frozen `AstCoreV1` ABI tag.
const ASTCORE_VERSION: &str = "v1";

/// Default in-memory capacity, in number of cached documents.
const DEFAULT_CAPACITY: usize = 1024;

/// BLAKE3 derive-key contexts. Distinct strings put the cache key and the config fingerprint in
/// separate hash domains so they can never alias; the version suffix doubles as a domain version.
const KEY_CONTEXT: &str = "tsuzulint document-cache key v1";
const CONFIG_CONTEXT: &str = "tsuzulint config fingerprint v1";

/// A 32-byte BLAKE3 digest identifying a `(content, config, versions, …)` tuple.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CacheKey([u8; 32]);

impl CacheKey {
    /// The raw digest bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex encoding (64 chars). Reserved for future on-disk cache filenames (M1g).
    pub fn to_hex(&self) -> String {
        let mut hex = String::with_capacity(64);
        for byte in &self.0 {
            // `from_digit` is infallible for radix 16 and a nibble (0..=15); the fallback is
            // unreachable and keeps this panic-free.
            hex.push(char::from_digit((byte >> 4) as u32, 16).unwrap_or('0'));
            hex.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap_or('0'));
        }
        hex
    }
}

/// Append a variable-length field, length-prefixed (u64 LE), so distinct field sequences cannot
/// collide by re-segmentation.
fn put(hasher: &mut Hasher, field: &[u8]) {
    hasher.update(&(field.len() as u64).to_le_bytes());
    hasher.update(field);
}

/// The inputs that fully determine a document's lint result.
pub struct CacheKeyInput<'a> {
    /// The exact document source. It MUST be the same `&str` later handed to `parse` (see
    /// [`lint_cached`]), so the key and the parsed spans describe the same bytes.
    pub content: &'a str,
    /// The resolved configuration.
    pub config: &'a Config,
    /// `(rule-id, rule-version)` for the enabled rules, in any order (sorted internally). Empty
    /// until the rules crate (M1f) supplies real versions, with no key-shape change.
    pub rule_versions: &'a [(&'a str, &'a str)],
    /// Morphology-dictionary fingerprint (M2). Empty until morphology lands; an empty slice is
    /// encoded as "kind = none", so a non-empty fingerprint can never collide with a key built
    /// without a dictionary.
    pub morphology_fingerprint: &'a [u8],
}

/// Build the document cache key. The only fallible step is canonicalizing rule `options` to
/// JSON bytes; it is propagated rather than unwrapped.
pub fn document_cache_key(input: &CacheKeyInput) -> Result<CacheKey, serde_json::Error> {
    let mut hasher = Hasher::new_derive_key(KEY_CONTEXT);
    // 1. Key-schema version (fixed width; no length prefix needed).
    hasher.update(&KEY_SCHEMA_VERSION.to_le_bytes());
    // 2. Content, hashed in the default BLAKE3 domain (distinct from the derive-key domain).
    put(
        &mut hasher,
        blake3::hash(input.content.as_bytes()).as_bytes(),
    );
    // 3. Config fingerprint.
    put(&mut hasher, &fingerprint_config(input.config)?);
    // 4. Parser/transform/engine/ABI versions, as one canonical string.
    let version_blob = format!(
        "parser={PARSER_VERSION};transform={AST_TRANSFORM_VERSION};\
         engine={ENGINE_BEHAVIOR_VERSION};astcore={ASTCORE_VERSION}"
    );
    put(&mut hasher, version_blob.as_bytes());
    // 5. Rule versions, sorted so order is irrelevant.
    let mut rule_versions = input.rule_versions.to_vec();
    rule_versions.sort_unstable();
    hasher.update(&(rule_versions.len() as u64).to_le_bytes());
    for (id, version) in rule_versions {
        put(&mut hasher, id.as_bytes());
        put(&mut hasher, version.as_bytes());
    }
    // 6. Morphology: a kind discriminant (0 = none) then the fingerprint bytes.
    let kind: u8 = u8::from(!input.morphology_fingerprint.is_empty());
    hasher.update(&[kind]);
    put(&mut hasher, input.morphology_fingerprint);

    Ok(CacheKey(*hasher.finalize().as_bytes()))
}

/// Canonical 32-byte fingerprint of a [`Config`].
///
/// `Config` can't derive `Hash` (its `options` are `serde_json::Value`), so build a
/// deterministic byte image and hash it. Rule entries iterate in sorted order (the map is a
/// `BTreeMap`), and `serde_json::to_vec` emits object keys in sorted order (serde_json has no
/// `preserve_order` feature here — a CI test pins this), so the image is canonical.
fn fingerprint_config(config: &Config) -> Result<[u8; 32], serde_json::Error> {
    let mut hasher = Hasher::new_derive_key(CONFIG_CONTEXT);
    put_opt_str(&mut hasher, config.language.as_deref());
    // `message_language` changes diagnostic message *text*, so it must be in the key.
    put_opt_str(&mut hasher, config.message_language.as_deref());

    hasher.update(&(config.rules.len() as u64).to_le_bytes());
    for (id, setting) in &config.rules {
        put(&mut hasher, id.as_str().as_bytes());
        match setting {
            RuleSetting::Off => {
                hasher.update(&[0x00]);
            }
            RuleSetting::On { severity, options } => {
                hasher.update(&[0x01]);
                match severity {
                    None => {
                        hasher.update(&[0x00]);
                    }
                    Some(sev) => {
                        hasher.update(&[0x01, severity_byte(*sev)]);
                    }
                }
                put(&mut hasher, &serde_json::to_vec(options)?);
            }
        }
    }
    Ok(*hasher.finalize().as_bytes())
}

/// Encode an optional string with a present/absent tag so `None` and `Some("")` differ.
fn put_opt_str(hasher: &mut Hasher, value: Option<&str>) {
    match value {
        None => {
            hasher.update(&[0x00]);
        }
        Some(s) => {
            hasher.update(&[0x01]);
            put(hasher, s.as_bytes());
        }
    }
}

/// Stable severity discriminant for the fingerprint.
fn severity_byte(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
        Severity::Hint => 3,
    }
}

/// A bounded, process-local, in-memory cache from [`CacheKey`] to a document's diagnostics.
///
/// Least-recently-used eviction keeps it to a fixed number of documents. It is `!Sync`
/// (single-owner); a multi-threaded caller wraps it in its own lock. Stored entries are cloned
/// on insert and on read, so a returned `Vec` can never alias (and mutate) a cached one.
pub struct DocumentCache {
    entries: HashMap<CacheKey, Vec<Diagnostic>>,
    /// Recency order: front = least-recently-used, back = most-recently-used.
    recency: VecDeque<CacheKey>,
    capacity: usize,
}

impl DocumentCache {
    /// A cache with the default capacity ([`DEFAULT_CAPACITY`] documents).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// A cache holding at most `capacity` documents (clamped to at least 1).
    pub fn with_capacity(capacity: usize) -> Self {
        DocumentCache {
            entries: HashMap::new(),
            recency: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    /// Look up `key`, returning a clone and marking it most-recently-used. Takes `&mut self`
    /// because a read updates recency.
    pub fn get(&mut self, key: &CacheKey) -> Option<Vec<Diagnostic>> {
        let hit = self.entries.get(key).cloned()?;
        self.mark_recent(key);
        Some(hit)
    }

    /// Insert (or replace) the diagnostics for `key`, evicting the least-recently-used entries
    /// if over capacity.
    pub fn insert(&mut self, key: CacheKey, diagnostics: Vec<Diagnostic>) {
        if self.entries.insert(key, diagnostics).is_some() {
            self.mark_recent(&key); // replaced an existing entry
            return;
        }
        self.recency.push_back(key);
        while self.entries.len() > self.capacity {
            match self.recency.pop_front() {
                Some(lru) => {
                    self.entries.remove(&lru);
                }
                None => break,
            }
        }
    }

    /// Return the cached diagnostics for `key`, or compute them with `f`, cache, and return them.
    pub fn get_or_lint<F: FnOnce() -> Vec<Diagnostic>>(
        &mut self,
        key: CacheKey,
        f: F,
    ) -> Vec<Diagnostic> {
        if let Some(hit) = self.get(&key) {
            return hit;
        }
        let diagnostics = f();
        self.insert(key, diagnostics.clone());
        diagnostics
    }

    /// Number of cached documents.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Move `key` to the most-recently-used position.
    fn mark_recent(&mut self, key: &CacheKey) {
        if let Some(pos) = self.recency.iter().position(|k| k == key) {
            self.recency.remove(pos);
        }
        self.recency.push_back(*key);
    }
}

impl Default for DocumentCache {
    fn default() -> Self {
        Self::new()
    }
}

/// A failure while linting through the cache.
#[derive(Debug)]
pub enum CacheError {
    /// The document failed to parse. (Parse failures are not cached — the parse-error
    /// diagnostic format is defined alongside the CLI, M1g.)
    Parse(ParseError),
    /// Building the cache key failed (canonicalizing rule `options` to JSON).
    Key(serde_json::Error),
    /// Archiving/accessing the parsed AST failed (an internal round-trip that should not occur
    /// for a freshly-parsed document); surfaced rather than served as a fake empty hit.
    Archive(String),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheError::Parse(e) => write!(f, "{e}"),
            CacheError::Key(e) => write!(f, "cache key error: {e}"),
            CacheError::Archive(m) => write!(f, "archive error: {m}"),
        }
    }
}

impl std::error::Error for CacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CacheError::Parse(e) => Some(e),
            CacheError::Key(e) => Some(e),
            CacheError::Archive(_) => None,
        }
    }
}

/// Lint `content` under `config` with `rules`, using `cache` to skip parse+lint on a repeat.
///
/// The key is built from the *same* `content` that is parsed on a miss (so a hit's diagnostics
/// describe exactly those bytes) and from the actual `rules` — their ids, derived here — so a
/// different rule set never reuses another's entry. On a miss this parses, archives, accesses,
/// and runs [`Engine::lint`], then caches the result. Parse and archive failures are surfaced
/// (never stored as a fake empty hit).
///
/// Rule *versions* are not in the key yet (the [`Rule`](tzlint_pdk::Rule) trait carries no
/// version): two different implementations sharing an id are distinguished only by
/// `ENGINE_BEHAVIOR_VERSION` until a per-rule version lands, at which point it joins the
/// `(id, version)` pairs derived below.
pub fn lint_cached(
    cache: &mut DocumentCache,
    content: &str,
    config: &Config,
    rules: &[&dyn Rule],
) -> Result<Vec<Diagnostic>, CacheError> {
    // Derive rule identity from the rules actually run, so the key reflects the real rule set.
    // (Keying off a separately-supplied list would let it drift from `rules` — the staleness
    // this avoids: e.g. an empty-rules result must not be reused for a non-empty rule set.)
    let rule_versions: Vec<(&str, &str)> = rules
        .iter()
        .map(|rule| (rule.meta().id.as_str(), ""))
        .collect();
    let key = document_cache_key(&CacheKeyInput {
        content,
        config,
        rule_versions: &rule_versions,
        morphology_fingerprint: &[],
    })
    .map_err(CacheError::Key)?;

    if let Some(hit) = cache.get(&key) {
        return Ok(hit);
    }

    let ast = parse(content).map_err(CacheError::Parse)?;
    let bytes = tzlint_ast::to_archive(&ast).map_err(|e| CacheError::Archive(e.to_string()))?;
    let archived = tzlint_ast::access(&bytes).map_err(|e| CacheError::Archive(e.to_string()))?;
    let diagnostics = Engine::lint(archived, rules);
    cache.insert(key, diagnostics.clone());
    Ok(diagnostics)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tzlint_ast::{NodeKind, Span};
    use tzlint_pdk::{Context, NodeRef, RuleMeta};

    fn cfg(json: &str) -> Config {
        Config::parse(json, crate::config::ConfigFormat::Json).unwrap()
    }

    fn key_of(content: &str, config: &Config) -> CacheKey {
        document_cache_key(&CacheKeyInput {
            content,
            config,
            rule_versions: &[],
            morphology_fingerprint: &[],
        })
        .unwrap()
    }

    /// A stub rule that reports once per matching node, so `lint_cached` yields non-empty output.
    struct FlagKind {
        meta: RuleMeta,
    }
    impl FlagKind {
        fn new(kind: NodeKind) -> Self {
            FlagKind {
                meta: RuleMeta::new("flag", Severity::Warning, vec![kind]),
            }
        }
    }
    impl Rule for FlagKind {
        fn meta(&self) -> &RuleMeta {
            &self.meta
        }
        fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
            cx.report(node.span(), "flagged");
        }
    }

    #[test]
    fn key_is_deterministic() {
        let c = cfg(r#"{"language":"ja"}"#);
        assert_eq!(key_of("# H\n", &c), key_of("# H\n", &c));
    }

    #[test]
    fn key_is_content_sensitive() {
        let c = Config::default();
        assert_ne!(key_of("# H\n", &c), key_of("# I\n", &c));
        // A leading BOM is different content → a different key (parse strips it, but the key is
        // over the bytes the caller passes; lint_cached uses the same &str for both).
        assert_ne!(key_of("x", &c), key_of("\u{feff}x", &c));
    }

    #[test]
    fn key_is_config_sensitive() {
        let base = Config::default();
        assert_ne!(
            key_of("x", &base),
            key_of("x", &cfg(r#"{"language":"ja"}"#))
        );
        // message-language alone changes the key.
        assert_ne!(
            key_of("x", &cfg(r#"{"language":"ja"}"#)),
            key_of("x", &cfg(r#"{"language":"ja","message-language":"en"}"#))
        );
        // rule on/off, severity, and options each change it.
        assert_ne!(
            key_of("x", &cfg(r#"{"rules":{"r":true}}"#)),
            key_of("x", &cfg(r#"{"rules":{"r":false}}"#))
        );
        assert_ne!(
            key_of("x", &cfg(r#"{"rules":{"r":{"severity":"error"}}}"#)),
            key_of("x", &cfg(r#"{"rules":{"r":{"severity":"warning"}}}"#))
        );
        // info vs hint too (exercises every severity discriminant).
        assert_ne!(
            key_of("x", &cfg(r#"{"rules":{"r":{"severity":"info"}}}"#)),
            key_of("x", &cfg(r#"{"rules":{"r":{"severity":"hint"}}}"#))
        );
        assert_ne!(
            key_of("x", &cfg(r#"{"rules":{"r":{"options":{"max":1}}}}"#)),
            key_of("x", &cfg(r#"{"rules":{"r":{"options":{"max":2}}}}"#))
        );
    }

    #[test]
    fn key_is_invariant_to_option_key_order() {
        // The canonical JSON fingerprint must not depend on object-key order.
        let a = cfg(r#"{"rules":{"r":{"options":{"a":1,"b":2}}}}"#);
        let b = cfg(r#"{"rules":{"r":{"options":{"b":2,"a":1}}}}"#);
        assert_eq!(key_of("x", &a), key_of("x", &b));
    }

    #[test]
    fn serde_json_emits_sorted_object_keys() {
        // Pins the canonicalization premise: a future `preserve_order` activation would fail here.
        let bytes = serde_json::to_vec(&serde_json::json!({"b": 1, "a": 2})).unwrap();
        assert!(
            bytes.starts_with(br#"{"a"#),
            "serde_json must sort object keys"
        );
    }

    #[test]
    fn rule_versions_are_order_independent_and_significant() {
        let c = Config::default();
        let k = |rv: &[(&str, &str)]| {
            document_cache_key(&CacheKeyInput {
                content: "x",
                config: &c,
                rule_versions: rv,
                morphology_fingerprint: &[],
            })
            .unwrap()
        };
        assert_eq!(k(&[("b", "1"), ("a", "1")]), k(&[("a", "1"), ("b", "1")]));
        assert_ne!(k(&[]), k(&[("r", "1")]));
        assert_ne!(k(&[("r", "1")]), k(&[("r", "2")]));
    }

    #[test]
    fn morphology_kind_byte_forward_compat() {
        let c = Config::default();
        let none = document_cache_key(&CacheKeyInput {
            content: "x",
            config: &c,
            rule_versions: &[],
            morphology_fingerprint: &[],
        })
        .unwrap();
        let present = document_cache_key(&CacheKeyInput {
            content: "x",
            config: &c,
            rule_versions: &[],
            morphology_fingerprint: &[0xAB; 32],
        })
        .unwrap();
        assert_ne!(
            none, present,
            "an M2 dictionary must invalidate pre-M2 keys"
        );
    }

    #[test]
    fn to_hex_and_as_bytes() {
        let key = key_of("x", &Config::default());
        assert_eq!(key.as_bytes().len(), 32);
        let hex = key.to_hex();
        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        // First hex pair encodes the first byte.
        let first = format!("{:02x}", key.as_bytes()[0]);
        assert!(hex.starts_with(&first));
    }

    #[test]
    fn cache_get_insert_and_purity() {
        let mut cache = DocumentCache::new();
        let key = key_of("x", &Config::default());
        assert!(cache.get(&key).is_none());
        let diag = Diagnostic::new("r", Severity::Warning, Span::new(0, 1), "m");
        cache.insert(key, vec![diag.clone()]);
        let mut hit = cache.get(&key).unwrap();
        assert_eq!(hit, vec![diag.clone()]);
        // Mutating the returned Vec must not corrupt the stored entry.
        hit.clear();
        assert_eq!(cache.get(&key).unwrap(), vec![diag]);
    }

    #[test]
    fn cache_evicts_least_recently_used() {
        let mut cache = DocumentCache::with_capacity(2);
        let c = Config::default();
        let (k1, k2, k3) = (key_of("1", &c), key_of("2", &c), key_of("3", &c));
        cache.insert(k1, vec![]);
        cache.insert(k2, vec![]);
        // Touch k1 so k2 becomes the LRU.
        assert!(cache.get(&k1).is_some());
        cache.insert(k3, vec![]); // evicts k2
        assert!(cache.get(&k1).is_some());
        assert!(cache.get(&k2).is_none(), "k2 should have been evicted");
        assert!(cache.get(&k3).is_some());
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn cache_insert_replaces_existing_entry() {
        let mut cache = DocumentCache::with_capacity(4);
        let key = key_of("x", &Config::default());
        cache.insert(key, vec![]);
        let diag = Diagnostic::new("r", Severity::Info, Span::new(0, 1), "m");
        cache.insert(key, vec![diag.clone()]); // replace, not a second entry
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&key).unwrap(), vec![diag]);
    }

    #[test]
    fn get_or_lint_computes_then_caches() {
        let mut cache = DocumentCache::default(); // also exercises Default
        let key = key_of("x", &Config::default());
        let diag = Diagnostic::new("r", Severity::Hint, Span::new(2, 3), "m");
        let want = vec![diag];

        let mut calls = 0;
        let first = cache.get_or_lint(key, || {
            calls += 1;
            want.clone()
        });
        assert_eq!(first, want);
        // Second call hits the cache and does NOT recompute.
        let second = cache.get_or_lint(key, || {
            calls += 1;
            Vec::new()
        });
        assert_eq!(second, want);
        assert_eq!(calls, 1, "the closure must run only on the miss");
    }

    #[test]
    fn cache_error_key_arm_display_and_source() {
        use std::error::Error;
        // A serde_json error stands in for the key-build failure arm.
        let json_err = serde_json::from_str::<i32>("not a number").unwrap_err();
        let err = CacheError::Key(json_err);
        assert!(err.to_string().starts_with("cache key error:"));
        assert!(err.source().is_some());
    }

    #[test]
    fn lint_cached_miss_then_hit_matches_fresh() {
        let mut cache = DocumentCache::new();
        let c = Config::default();
        let rule = FlagKind::new(NodeKind::HEADING);
        let rules: &[&dyn Rule] = &[&rule];

        let fresh = lint_cached(&mut cache, "# H\n\nbody", &c, rules).unwrap();
        assert_eq!(fresh.len(), 1, "one heading flagged");
        assert_eq!(cache.len(), 1);
        // Second call is a hit returning identical diagnostics.
        let hit = lint_cached(&mut cache, "# H\n\nbody", &c, rules).unwrap();
        assert_eq!(hit, fresh);
        assert_eq!(cache.len(), 1, "no new entry on a hit");
    }

    #[test]
    fn lint_cached_keys_on_the_actual_rule_set() {
        // Regression: a result computed with one rule set must never be reused for another.
        let mut cache = DocumentCache::new();
        let c = Config::default();
        let rule = FlagKind::new(NodeKind::HEADING);

        let empty = lint_cached(&mut cache, "# H\n", &c, &[]).unwrap();
        assert!(empty.is_empty());
        // A different rule set must miss (not reuse the empty-rules entry) and produce its own.
        let with_rule = lint_cached(&mut cache, "# H\n", &c, &[&rule]).unwrap();
        assert_eq!(
            with_rule.len(),
            1,
            "different rules must not hit the empty entry"
        );
        assert_eq!(
            cache.len(),
            2,
            "distinct rule sets → distinct cache entries"
        );
    }

    #[test]
    fn lint_cached_empty_rules_is_no_diagnostics() {
        let mut cache = DocumentCache::new();
        let diags = lint_cached(&mut cache, "# H\n", &Config::default(), &[]).unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn lint_cached_surfaces_parse_error_without_caching() {
        let mut cache = DocumentCache::new();
        // Deeply nested block containers make `parse` reject the input.
        let pathological = "> ".repeat(5000);
        let err = lint_cached(&mut cache, &pathological, &Config::default(), &[]).unwrap_err();
        assert!(matches!(err, CacheError::Parse(_)));
        assert!(
            cache.is_empty(),
            "a parse failure must not populate the cache"
        );
    }

    #[test]
    fn cache_error_display_and_source() {
        use std::error::Error;
        let parse = CacheError::Parse(ParseError {
            message: "boom".into(),
        });
        assert!(parse.to_string().contains("boom"));
        assert!(parse.source().is_some());
        let archive = CacheError::Archive("x".into());
        assert_eq!(archive.to_string(), "archive error: x");
        assert!(archive.source().is_none());
    }
}
