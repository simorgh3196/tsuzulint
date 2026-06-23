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
use std::path::Path;

use blake3::Hasher;
use serde_json::Value;
use tzlint_ast::Span;
use tzlint_pdk::{Diagnostic, Fix, Severity};

use crate::config::{Config, RuleSetting};
use crate::io::{Host, IoError};
use crate::parse::ParseError;

/// Cache-key recipe version. Bump to invalidate every key if the key *construction* changes.
const KEY_SCHEMA_VERSION: u32 = 2;
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

/// On-disk cache file layout version. A file tagged with a different version is ignored on load
/// (treated as empty), so an old cache can never feed mis-shaped data to a new reader.
const CACHE_FILE_VERSION: u64 = 1;
/// Upper bound on the on-disk cache file size read back. Generous; a larger (or unreadable) file
/// is ignored rather than erroring — a cache must never break linting.
const MAX_CACHE_FILE: usize = 64 * 1024 * 1024;

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

    /// Lowercase hex encoding (64 chars). Used as the key in the on-disk cache file.
    pub fn to_hex(&self) -> String {
        blake3::Hash::from(self.0).to_hex().to_string()
    }

    /// Parse a 64-char lowercase-hex digest (as produced by [`to_hex`](CacheKey::to_hex)) back
    /// into a key, or `None` if it is not exactly 64 ASCII hex characters.
    pub fn from_hex(hex: &str) -> Option<CacheKey> {
        blake3::Hash::from_hex(hex)
            .ok()
            .map(|h| CacheKey(*h.as_bytes()))
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
    /// A stable identity for the processor that parses `content` — its primary extension
    /// (`"md"`, `"csv"`, `"tsv"`, …). Two processors can yield different diagnostics for
    /// byte-identical content (e.g. Markdown vs an un-configured CSV that lints nothing), so the
    /// key must distinguish them even when the resolved rule set coincides (spec §10).
    pub processor: &'a str,
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
    // 2b. Processor identity — byte-identical content under a different processor must not share
    // a key (see `CacheKeyInput::processor`).
    put(&mut hasher, input.processor.as_bytes());
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

    // Format settings: which columns are linted, with what parse mode and rule overlay.
    hasher.update(&(config.formats.len() as u64).to_le_bytes());
    for (fmt_id, fmt) in &config.formats {
        put(&mut hasher, fmt_id.as_bytes());
        hasher.update(&[u8::from(fmt.has_header)]);
        match fmt.delimiter {
            None => {
                hasher.update(&[0x00]);
            }
            Some(c) => {
                hasher.update(&[0x01]);
                put(&mut hasher, c.to_string().as_bytes());
            }
        }
        hasher.update(&(fmt.columns.len() as u64).to_le_bytes());
        for col in &fmt.columns {
            match &col.selector {
                crate::processor::ColumnSelector::Name(n) => {
                    hasher.update(&[0x00]);
                    put(&mut hasher, n.as_bytes());
                }
                crate::processor::ColumnSelector::Index(i) => {
                    hasher.update(&[0x01]);
                    hasher.update(&i.to_le_bytes());
                }
            }
            hasher.update(&[parse_mode_byte(col.parse_mode)]);
            // Column rule overlay (same encoding as the base rules above).
            hasher.update(&(col.rules.len() as u64).to_le_bytes());
            for (id, setting) in &col.rules {
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

/// Stable parse-mode discriminant for the format-settings fold in the fingerprint.
fn parse_mode_byte(mode: crate::processor::ParseMode) -> u8 {
    match mode {
        crate::processor::ParseMode::Markdown => 0,
        crate::processor::ParseMode::PlainText => 1,
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

    /// Load a cache from the JSON file at `path` through `host`, for cross-run reuse.
    ///
    /// **Best-effort:** a missing, oversized, malformed, or wrong-version file yields an empty
    /// cache rather than an error — a cache must never break linting. Individual unreadable
    /// entries are skipped. Each surviving key is content-/config-/version-addressed (see
    /// [`document_cache_key`]), so a stale entry is simply never looked up; only the byte ceiling
    /// and LRU capacity bound growth.
    pub fn load(host: &dyn Host, path: &Path) -> DocumentCache {
        let mut cache = DocumentCache::new();
        let Ok(text) = host.read_to_string(path, MAX_CACHE_FILE) else {
            return cache;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return cache;
        };
        if value.get("version").and_then(Value::as_u64) != Some(CACHE_FILE_VERSION) {
            return cache;
        }
        let Some(entries) = value.get("entries").and_then(Value::as_object) else {
            return cache;
        };
        for (hex, diagnostics_value) in entries {
            let (Some(key), Some(array)) = (CacheKey::from_hex(hex), diagnostics_value.as_array())
            else {
                continue;
            };
            let diagnostics: Option<Vec<Diagnostic>> =
                array.iter().map(diagnostic_from_value).collect();
            if let Some(diagnostics) = diagnostics {
                cache.insert(key, diagnostics);
            }
        }
        cache
    }

    /// Persist the cache to the JSON file at `path` through `host` (atomic write). The file is a
    /// `{ "version", "entries": { "<hex-key>": [<diagnostic>…] } }` document.
    pub fn save(&self, host: &dyn Host, path: &Path) -> Result<(), CacheError> {
        self.save_within(host, path, MAX_CACHE_FILE)
    }

    /// Persist the cache, rejecting a serialized document larger than `limit` bytes *before*
    /// writing. The cap mirrors [`load`](DocumentCache::load)'s read limit, so `save` never
    /// reports success for a file the next `load` would silently discard as oversized; checking
    /// the in-memory bytes before the write keeps it TOCTOU-free. (`MAX_CACHE_FILE` in practice;
    /// the parameter lets tests drive the bound without a 64 MiB fixture.)
    fn save_within(&self, host: &dyn Host, path: &Path, limit: usize) -> Result<(), CacheError> {
        let mut entries = serde_json::Map::with_capacity(self.entries.len());
        for (key, diagnostics) in &self.entries {
            let array: Vec<Value> = diagnostics.iter().map(diagnostic_to_value).collect();
            entries.insert(key.to_hex(), Value::Array(array));
        }
        let document = serde_json::json!({
            "version": CACHE_FILE_VERSION,
            "entries": Value::Object(entries),
        });
        let text = serde_json::to_string(&document).unwrap_or_default();
        if text.len() > limit {
            return Err(CacheError::Io(IoError::TooLarge { limit }));
        }
        host.write_atomic(path, text.as_bytes())
            .map_err(CacheError::Io)
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
    /// Writing the on-disk cache file failed (from [`DocumentCache::save`]).
    Io(IoError),
}

impl fmt::Display for CacheError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CacheError::Parse(e) => write!(f, "{e}"),
            CacheError::Key(e) => write!(f, "cache key error: {e}"),
            CacheError::Archive(m) => write!(f, "archive error: {m}"),
            CacheError::Io(e) => write!(f, "cache file error: {e}"),
        }
    }
}

impl std::error::Error for CacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CacheError::Parse(e) => Some(e),
            CacheError::Key(e) => Some(e),
            CacheError::Archive(_) => None,
            CacheError::Io(e) => Some(e),
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
///
/// `morphology` is the per-run provider registry (M2h). Its
/// [`fingerprint`](crate::MorphologyRegistry::fingerprint) of the dictionaries *active* for `rules`
/// is folded into the key, so a dictionary upgrade invalidates stale entries. `None` (or an empty
/// registry, or a registry whose languages no enabled rule needs) yields an empty fingerprint and a
/// **byte-identical** pre-morphology key. The fingerprint affects **keying only** — the miss path
/// still runs [`Engine::lint`] with no morphology table (the analysis pass is M2l), so registry
/// presence never changes diagnostics.
// Eight references, none of which bundle naturally (one is `&mut`, the rest are disjoint `&`s):
// the trailing `morphology: Option<&MorphologyRegistry>` is the deliberate, reviewed shape, so the
// arg-count lint is allowed here rather than forcing an artificial context struct.
#[allow(clippy::too_many_arguments)]
pub fn lint_cached(
    cache: &mut DocumentCache,
    ext: Option<&str>,
    content: &str,
    config: &Config,
    registry: &crate::Registry,
    processor_cfg: &crate::ProcessorConfig,
    rules: &crate::RegionRules,
    morphology: Option<&crate::MorphologyRegistry>,
) -> Result<Vec<Diagnostic>, CacheError> {
    // Derive rule identity from every rule across the base and column sets, so the key reflects
    // the real rule set. (Keying off a separately-supplied list would let it drift from `rules`
    // — the staleness this avoids: e.g. an empty-rules result must not be reused for a non-empty
    // rule set.) The `config` fold already covers the `formats` settings (Task 3.3).
    let rule_versions: Vec<(&str, &str)> =
        rules.rule_ids().into_iter().map(|id| (id, "")).collect();
    // The selected processor's primary extension identifies it in the key, so byte-identical
    // content linted under two processors (e.g. `.md` vs an un-configured `.csv`) never collides
    // even when `rule_ids()` coincide.
    let processor = registry
        .for_ext(ext)
        .extensions()
        .first()
        .copied()
        .unwrap_or("");
    // M2e: fold the active-dictionary fingerprint into the key. Empty (no registry / no active
    // dictionary) ⇒ a byte-identical pre-morphology key.
    let morphology_fingerprint: Vec<u8> = match morphology {
        Some(reg) => reg.fingerprint(rules),
        None => Vec::new(),
    };
    let key = document_cache_key(&CacheKeyInput {
        content,
        processor,
        config,
        rule_versions: &rule_versions,
        morphology_fingerprint: &morphology_fingerprint,
    })
    .map_err(CacheError::Key)?;

    if let Some(hit) = cache.get(&key) {
        return Ok(hit);
    }

    let diagnostics =
        crate::lint_document(ext, content, registry, processor_cfg, rules, morphology)
            .map_err(CacheError::Parse)?;
    cache.insert(key, diagnostics.clone());
    Ok(diagnostics)
}

/// The on-disk wire name for a severity (round-trips with [`severity_from_name`]).
fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
    }
}

/// Parse a severity wire name, or `None` if unrecognized.
fn severity_from_name(name: &str) -> Option<Severity> {
    match name {
        "error" => Some(Severity::Error),
        "warning" => Some(Severity::Warning),
        "info" => Some(Severity::Info),
        "hint" => Some(Severity::Hint),
        _ => None,
    }
}

/// Serialize one diagnostic to the cache-file JSON shape.
fn diagnostic_to_value(diagnostic: &Diagnostic) -> Value {
    serde_json::json!({
        "rule_id": diagnostic.rule_id.as_str(),
        "severity": severity_name(diagnostic.severity),
        "message": diagnostic.message,
        "span": { "start": diagnostic.span.start, "end": diagnostic.span.end },
        "fixes": diagnostic
            .fixes
            .iter()
            .map(|fix| serde_json::json!({
                "span": { "start": fix.span.start, "end": fix.span.end },
                "replacement": fix.replacement,
            }))
            .collect::<Vec<_>>(),
    })
}

/// Reconstruct a diagnostic from the cache-file JSON shape, or `None` if any field is missing or
/// malformed (the caller drops the whole entry, so a corrupt cache degrades to a miss).
fn diagnostic_from_value(value: &Value) -> Option<Diagnostic> {
    let rule_id = value.get("rule_id")?.as_str()?;
    let severity = severity_from_name(value.get("severity")?.as_str()?)?;
    let message = value.get("message")?.as_str()?;
    let span = span_from_value(value.get("span")?)?;
    let mut diagnostic = Diagnostic::new(rule_id, severity, span, message);
    // A present-but-non-array `fixes` is corrupt: fail (so the caller drops the whole
    // diagnostic) rather than silently accept it without fixes, which would diverge from a
    // fresh lint. An absent `fixes` key is fine — the diagnostic simply has none.
    if let Some(fixes) = value.get("fixes") {
        for fix in fixes.as_array()? {
            let span = span_from_value(fix.get("span")?)?;
            let replacement = fix.get("replacement")?.as_str()?;
            diagnostic.fixes.push(Fix::replace(span, replacement));
        }
    }
    Some(diagnostic)
}

/// Parse a `{ "start", "end" }` object into a [`Span`], or `None` if out of `u32` range.
fn span_from_value(value: &Value) -> Option<Span> {
    let start = u32::try_from(value.get("start")?.as_u64()?).ok()?;
    let end = u32::try_from(value.get("end")?.as_u64()?).ok()?;
    Some(Span::new(start, end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tzlint_ast::{NodeKind, Span};
    use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta};

    fn cfg(json: &str) -> Config {
        Config::parse(json, crate::config::ConfigFormat::Json).unwrap()
    }

    fn key_of(content: &str, config: &Config) -> CacheKey {
        document_cache_key(&CacheKeyInput {
            content,
            processor: "md",
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

    /// A morphology-requiring rule (pinned to JA): the engine skips it when no table is available
    /// (`lint_document` passes `None`), so it never reports — its purpose is to put a `required_lang`
    /// into the rule set so the morphology fingerprint becomes active.
    struct MorphFlag {
        meta: RuleMeta,
    }
    impl MorphFlag {
        fn new(id: &str) -> Self {
            MorphFlag {
                meta: RuleMeta::new(id, Severity::Warning, vec![NodeKind::HEADING])
                    .with_morphology(tzlint_ast::morphology::Lang::JA),
            }
        }
    }
    impl Rule for MorphFlag {
        fn meta(&self) -> &RuleMeta {
            &self.meta
        }
        fn check<'ast>(&self, node: NodeRef<'ast>, cx: &mut Context<'ast>) {
            cx.report(node.span(), "morph");
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
    fn formats_change_the_cache_key() {
        use crate::processor::{ColumnSelector, ParseMode};
        use crate::{ColumnConfig, Config, FormatConfig};
        use std::collections::BTreeMap;

        let base = Config::default();
        let mut with_cols = Config::default();
        let mut formats = BTreeMap::new();
        formats.insert(
            "csv".to_string(),
            FormatConfig {
                has_header: true,
                delimiter: None,
                columns: vec![ColumnConfig {
                    selector: ColumnSelector::Name("body".into()),
                    parse_mode: ParseMode::Markdown,
                    rules: BTreeMap::new(),
                }],
            },
        );
        with_cols.formats = formats;

        let k1 = key_of("x", &base);
        let k2 = key_of("x", &with_cols);
        assert_ne!(k1, k2);
    }

    #[test]
    fn processor_identity_is_in_the_cache_key() {
        // Byte-identical content + config under two processors yields different keys (spec §10).
        let config = Config::default();
        let key = |processor| {
            document_cache_key(&CacheKeyInput {
                content: "x",
                processor,
                config: &config,
                rule_versions: &[],
                morphology_fingerprint: &[],
            })
            .unwrap()
        };
        assert_ne!(key("md"), key("csv"));
        assert_eq!(key("md"), key("md"));
    }

    #[test]
    fn csv_without_columns_does_not_reuse_markdown_cache_entry() {
        // Regression: the same content + config (no `formats`) linted as `.md` then as `.csv`
        // must NOT share a cache entry. Markdown flags a TEXT node; an un-configured CSV has no
        // target columns, so it lints nothing. Their `rule_ids()` coincide (base-only), so before
        // the processor identity was folded into the key the CSV wrongly reused the Markdown hit.
        use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity};

        struct FlagText(RuleMeta);
        impl Rule for FlagText {
            fn meta(&self) -> &RuleMeta {
                &self.0
            }
            fn check<'a>(&self, node: NodeRef<'a>, cx: &mut Context<'a>) {
                cx.report(node.span(), "text");
            }
        }

        let rules = crate::RegionRules::base_only(vec![Box::new(FlagText(RuleMeta::new(
            "flag-text",
            Severity::Warning,
            vec![tzlint_ast::NodeKind::TEXT],
        )))]);
        let registry = crate::Registry::with_builtins();
        let pcfg = crate::ProcessorConfig::default();
        let config = Config::default();
        let mut cache = DocumentCache::new();

        let md = lint_cached(
            &mut cache,
            Some("md"),
            "abc",
            &config,
            &registry,
            &pcfg,
            &rules,
            None,
        )
        .unwrap();
        assert!(!md.is_empty(), "markdown should flag the text node");

        let csv = lint_cached(
            &mut cache,
            Some("csv"),
            "abc",
            &config,
            &registry,
            &pcfg,
            &rules,
            None,
        )
        .unwrap();
        assert!(
            csv.is_empty(),
            "an un-configured csv must lint nothing, not reuse the markdown cache entry"
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
                processor: "md",
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
            processor: "md",
            config: &c,
            rule_versions: &[],
            morphology_fingerprint: &[],
        })
        .unwrap();
        let present = document_cache_key(&CacheKeyInput {
            content: "x",
            processor: "md",
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
    fn key_schema_version_is_unchanged() {
        // Guards against silent whole-key drift; the no-morphology path must stay pre-M2.
        assert_eq!(KEY_SCHEMA_VERSION, 2);
    }

    #[test]
    fn lint_cached_none_and_empty_registry_are_pre_m2_byte_identical() {
        use crate::MorphologyRegistry;
        let mut cache = DocumentCache::new();
        let c = Config::default();
        let registry = crate::Registry::with_builtins();
        let pcfg = crate::ProcessorConfig::default();
        // A morphology-requiring rule present but NO provider registered: `required_langs` is [JA]
        // yet the active set is empty, so the fingerprint must still be empty (a pre-M2 key). Using
        // a morphology rule makes a fresh MISS distinguishable from a HIT: on a miss `lint_document`
        // passes a None table, so MorphFlag is SKIPPED → 0 diagnostics; the seeded reference has 1.
        let rules = crate::RegionRules::base_only(vec![Box::new(MorphFlag::new("mf"))]);

        // The reference pre-M2 key: built directly with an empty morphology fingerprint.
        let processor = registry
            .for_ext(Some("md"))
            .extensions()
            .first()
            .copied()
            .unwrap_or("");
        let rule_versions: Vec<(&str, &str)> =
            rules.rule_ids().into_iter().map(|id| (id, "")).collect();
        let reference = document_cache_key(&CacheKeyInput {
            content: "# H\n",
            processor,
            config: &c,
            rule_versions: &rule_versions,
            morphology_fingerprint: &[],
        })
        .unwrap();

        // Seed the reference key with one marker diagnostic. If None / empty-registry compute the
        // SAME key they HIT and return the marker (len 1); any drift would MISS and recompute to
        // len 0 (MorphFlag skipped). So `len == 1` discriminates key-equality.
        cache.insert(
            reference,
            vec![Diagnostic::new("x", Severity::Info, Span::new(0, 0), "m")],
        );
        let none = lint_cached(
            &mut cache,
            Some("md"),
            "# H\n",
            &c,
            &registry,
            &pcfg,
            &rules,
            None,
        )
        .unwrap();
        assert_eq!(
            none.len(),
            1,
            "None must key-match the pre-M2 reference (hit, not recompute)"
        );
        assert_eq!(none[0].message, "m");
        let empty = MorphologyRegistry::new();
        let with_empty = lint_cached(
            &mut cache,
            Some("md"),
            "# H\n",
            &c,
            &registry,
            &pcfg,
            &rules,
            Some(&empty),
        )
        .unwrap();
        assert_eq!(
            with_empty.len(),
            1,
            "an empty registry must key-match the pre-M2 reference"
        );
        assert_eq!(with_empty[0].message, "m");
    }

    #[test]
    fn lint_cached_with_morphology_miss_then_hit() {
        use crate::{DictId, MorphologyRegistry};
        use tzlint_ast::morphology::Lang;
        use tzlint_pdk::WhitespaceProvider;
        let mut cache = DocumentCache::new();
        let c = Config::default();
        let registry = crate::Registry::with_builtins();
        let pcfg = crate::ProcessorConfig::default();
        // A rule that needs JA morphology, so the JA provider lands in the active set.
        let rules = crate::RegionRules::base_only(vec![Box::new(MorphFlag::new("mf"))]);

        let mut reg = MorphologyRegistry::new();
        reg.insert(
            Box::new(WhitespaceProvider::new(Lang::JA)),
            DictId::from_pin([0x9; 32]),
        );

        let miss = lint_cached(
            &mut cache,
            Some("md"),
            "# H\n",
            &c,
            &registry,
            &pcfg,
            &rules,
            Some(&reg),
        )
        .unwrap();
        assert_eq!(
            cache.len(),
            1,
            "miss caches one entry under the morphology-fingerprinted key"
        );
        // The analysis pass ran through lint_cached: MorphFlag saw the table and fired.
        assert_eq!(
            miss.iter().map(|d| d.message.as_str()).collect::<Vec<_>>(),
            vec!["morph"],
            "lint_cached must forward the registry so the morphology rule fires"
        );
        let hit = lint_cached(
            &mut cache,
            Some("md"),
            "# H\n",
            &c,
            &registry,
            &pcfg,
            &rules,
            Some(&reg),
        )
        .unwrap();
        assert_eq!(hit, miss, "second call hits the same key");
        assert_eq!(cache.len(), 1, "no new entry on a hit");
    }

    #[test]
    fn lint_cached_active_registry_misses_the_pre_m2_key() {
        // The slice's load-bearing behavior, asserted across the `lint_cached` boundary: an ACTIVE
        // registry must produce a DIFFERENT key than the pre-M2 (empty-fingerprint) key. This is
        // the inverse of `lint_cached_none_and_empty_registry_are_pre_m2_byte_identical`: seed the
        // pre-M2 key, then assert an active registry does NOT hit it. Kills the
        // `Some(_reg) => Vec::new()` mutant — which would collapse the fingerprint to empty, giving
        // the same key and a spurious HIT — that the miss-then-hit self-consistency test cannot.
        use crate::{DictId, MorphologyRegistry};
        use tzlint_ast::morphology::Lang;
        use tzlint_pdk::WhitespaceProvider;
        let mut cache = DocumentCache::new();
        let c = Config::default();
        let registry = crate::Registry::with_builtins();
        let pcfg = crate::ProcessorConfig::default();
        let rules = crate::RegionRules::base_only(vec![Box::new(MorphFlag::new("mf"))]);

        // Pre-M2 reference key (empty morphology fingerprint), seeded with a marker diagnostic.
        let processor = registry
            .for_ext(Some("md"))
            .extensions()
            .first()
            .copied()
            .unwrap_or("");
        let rule_versions: Vec<(&str, &str)> =
            rules.rule_ids().into_iter().map(|id| (id, "")).collect();
        let pre_m2 = document_cache_key(&CacheKeyInput {
            content: "# H\n",
            processor,
            config: &c,
            rule_versions: &rule_versions,
            morphology_fingerprint: &[],
        })
        .unwrap();
        cache.insert(
            pre_m2,
            vec![Diagnostic::new("x", Severity::Info, Span::new(0, 0), "m")],
        );

        // An active registry (a JA provider + the JA-needing MorphFlag rule) yields a non-empty
        // fingerprint, so the key differs from `pre_m2`: the call MISSES the seed and recomputes.
        // The analysis pass then feeds the table to `Engine::lint`, so MorphFlag FIRES ("morph") —
        // distinct from the seeded marker ("m") a hit would return. This kills two mutants at once:
        //   * fingerprint ignores the registry (`Some(_) => Vec::new()`) → same key → HIT → "m";
        //   * lint_cached drops the registry instead of forwarding it to lint_document → MISS but
        //     MorphFlag skipped (None table) → empty.
        let mut active = MorphologyRegistry::new();
        active.insert(
            Box::new(WhitespaceProvider::new(Lang::JA)),
            DictId::from_pin([0x9; 32]),
        );
        let out = lint_cached(
            &mut cache,
            Some("md"),
            "# H\n",
            &c,
            &registry,
            &pcfg,
            &rules,
            Some(&active),
        )
        .unwrap();
        assert_eq!(
            out.len(),
            1,
            "an active registry must MISS the seed and recompute"
        );
        assert_eq!(
            out[0].message, "morph",
            "the recompute must run morphology (MorphFlag fired), not return the seeded marker"
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
        let registry = crate::Registry::with_builtins();
        let pcfg = crate::ProcessorConfig::default();
        let new_rules =
            || crate::RegionRules::base_only(vec![Box::new(FlagKind::new(NodeKind::HEADING))]);

        let fresh = lint_cached(
            &mut cache,
            Some("md"),
            "# H\n\nbody",
            &c,
            &registry,
            &pcfg,
            &new_rules(),
            None,
        )
        .unwrap();
        assert_eq!(fresh.len(), 1, "one heading flagged");
        assert_eq!(cache.len(), 1);
        // Second call is a hit returning identical diagnostics.
        let hit = lint_cached(
            &mut cache,
            Some("md"),
            "# H\n\nbody",
            &c,
            &registry,
            &pcfg,
            &new_rules(),
            None,
        )
        .unwrap();
        assert_eq!(hit, fresh);
        assert_eq!(cache.len(), 1, "no new entry on a hit");
    }

    #[test]
    fn lint_cached_csv_round_trip_matches_fresh_compute() {
        use crate::processor::{
            ColumnSelector, ColumnTarget, DelimitedConfig, ParseMode, ProcessorConfig, RegionRules,
            Registry,
        };
        use crate::{ColumnConfig, FormatConfig};
        use std::collections::BTreeMap;

        // A FlagText-style rule that flags TEXT nodes, applied only to the "body" column.
        let source = "id,body\n1,hello\n2,world\n";
        let registry = Registry::with_builtins();
        let pcfg = ProcessorConfig {
            delimited: Some(DelimitedConfig {
                delimiter: b',',
                has_header: true,
                columns: vec![ColumnTarget {
                    selector: ColumnSelector::Name("body".into()),
                    parse_mode: ParseMode::PlainText,
                }],
            }),
        };
        // The config must carry the formats so the cache key reflects them.
        let mut formats = BTreeMap::new();
        formats.insert(
            "csv".to_string(),
            FormatConfig {
                has_header: true,
                delimiter: None,
                columns: vec![ColumnConfig {
                    selector: ColumnSelector::Name("body".into()),
                    parse_mode: ParseMode::PlainText,
                    rules: BTreeMap::new(),
                }],
            },
        );
        let config = Config {
            formats,
            ..Default::default()
        };

        let build_rules = || RegionRules::base_only(vec![Box::new(FlagKind::new(NodeKind::TEXT))]);

        let fresh =
            crate::lint_document(Some("csv"), source, &registry, &pcfg, &build_rules(), None)
                .unwrap();
        assert_eq!(fresh.len(), 2, "two body cells flagged");

        let mut cache = DocumentCache::new();
        let miss = lint_cached(
            &mut cache,
            Some("csv"),
            source,
            &config,
            &registry,
            &pcfg,
            &build_rules(),
            None,
        )
        .unwrap();
        assert_eq!(miss, fresh, "cached miss equals a fresh compute");
        assert_eq!(cache.len(), 1);
        let hit = lint_cached(
            &mut cache,
            Some("csv"),
            source,
            &config,
            &registry,
            &pcfg,
            &build_rules(),
            None,
        )
        .unwrap();
        assert_eq!(hit, fresh, "cached hit equals a fresh compute");
        assert_eq!(cache.len(), 1, "no new entry on a hit");
    }

    #[test]
    fn lint_cached_keys_on_the_actual_rule_set() {
        // Regression: a result computed with one rule set must never be reused for another.
        let mut cache = DocumentCache::new();
        let c = Config::default();
        let registry = crate::Registry::with_builtins();
        let pcfg = crate::ProcessorConfig::default();

        let empty = lint_cached(
            &mut cache,
            Some("md"),
            "# H\n",
            &c,
            &registry,
            &pcfg,
            &crate::RegionRules::base_only(vec![]),
            None,
        )
        .unwrap();
        assert!(empty.is_empty());
        // A different rule set must miss (not reuse the empty-rules entry) and produce its own.
        let with_rule = lint_cached(
            &mut cache,
            Some("md"),
            "# H\n",
            &c,
            &registry,
            &pcfg,
            &crate::RegionRules::base_only(vec![Box::new(FlagKind::new(NodeKind::HEADING))]),
            None,
        )
        .unwrap();
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
        let registry = crate::Registry::with_builtins();
        let diags = lint_cached(
            &mut cache,
            Some("md"),
            "# H\n",
            &Config::default(),
            &registry,
            &crate::ProcessorConfig::default(),
            &crate::RegionRules::base_only(vec![]),
            None,
        )
        .unwrap();
        assert!(diags.is_empty());
    }

    #[test]
    fn lint_cached_surfaces_parse_error_without_caching() {
        let mut cache = DocumentCache::new();
        let registry = crate::Registry::with_builtins();
        // Deeply nested block containers make `parse` reject the input.
        let pathological = "> ".repeat(5000);
        let err = lint_cached(
            &mut cache,
            Some("md"),
            &pathological,
            &Config::default(),
            &registry,
            &crate::ProcessorConfig::default(),
            &crate::RegionRules::base_only(vec![]),
            None,
        )
        .unwrap_err();
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
        let io = CacheError::Io(IoError::Other("disk full".into()));
        assert!(io.to_string().contains("cache file error"), "{io}");
        assert!(io.source().is_some());
    }

    /// A tiny in-memory [`Host`] for cache persistence tests.
    struct MemHost {
        files: std::cell::RefCell<std::collections::HashMap<std::path::PathBuf, String>>,
    }
    impl MemHost {
        fn new() -> Self {
            MemHost {
                files: std::cell::RefCell::new(std::collections::HashMap::new()),
            }
        }
        fn seed(path: &str, contents: &str) -> Self {
            let host = MemHost::new();
            host.files
                .borrow_mut()
                .insert(std::path::PathBuf::from(path), contents.to_string());
            host
        }
    }
    impl Host for MemHost {
        fn read_to_string(&self, path: &Path, limit: usize) -> Result<String, IoError> {
            match self.files.borrow().get(path) {
                Some(c) if c.len() > limit => Err(IoError::TooLarge { limit }),
                Some(c) => Ok(c.clone()),
                None => Err(IoError::NotFound),
            }
        }
        fn write_atomic(&self, path: &Path, contents: &[u8]) -> Result<(), IoError> {
            let text =
                String::from_utf8(contents.to_vec()).map_err(|e| IoError::Other(e.to_string()))?;
            self.files.borrow_mut().insert(path.to_path_buf(), text);
            Ok(())
        }
        fn exists(&self, path: &Path) -> bool {
            self.files.borrow().contains_key(path)
        }
    }

    #[test]
    fn cache_key_hex_round_trips() {
        let key = key_of("# H\n", &cfg(r#"{"language":"ja"}"#));
        assert_eq!(CacheKey::from_hex(&key.to_hex()), Some(key));
        // Malformed hex is rejected, never panics.
        assert_eq!(CacheKey::from_hex("not hex"), None);
        assert_eq!(CacheKey::from_hex(&"z".repeat(64)), None);
        assert_eq!(CacheKey::from_hex(&"a".repeat(63)), None);
    }

    #[test]
    fn cache_save_then_load_round_trips_entries() {
        let key = key_of("doc\n", &cfg("{}"));
        // All four severities, so `severity_name`/`severity_from_name` round-trip every arm.
        let diagnostics = vec![
            // Two fixes, so the fix (de)serialization loop runs more than one iteration.
            Diagnostic::new("max-ten", Severity::Error, Span::new(2, 5), "msg")
                .with_fix(Fix::replace(Span::new(2, 3), "x"))
                .with_fix(Fix::delete(Span::new(3, 5))),
            Diagnostic::new("no-todo", Severity::Warning, Span::new(0, 4), "todo"),
            Diagnostic::new("no-nfd", Severity::Info, Span::new(1, 2), "nfd"),
            Diagnostic::new("sentence-length", Severity::Hint, Span::new(3, 4), "len"),
        ];
        let mut cache = DocumentCache::new();
        cache.insert(key, diagnostics.clone());

        let host = MemHost::new();
        let path = Path::new("/work/.tzlintcache");
        cache.save(&host, path).unwrap();

        let loaded = DocumentCache::load(&host, path);
        assert_eq!(loaded.len(), 1);
        let mut loaded = loaded;
        assert_eq!(loaded.get(&key).as_deref(), Some(diagnostics.as_slice()));
    }

    #[test]
    fn cache_load_accepts_a_diagnostic_without_fixes() {
        // A diagnostic with no `fixes` array (the common case) loads fine — exercises the
        // fixes-absent path of the entry decoder.
        let hex = key_of("doc\n", &cfg("{}")).to_hex();
        let file = format!(
            r#"{{"version":1,"entries":{{"{hex}":[{{"rule_id":"r","severity":"warning","message":"m","span":{{"start":0,"end":1}}}}]}}}}"#
        );
        let loaded = DocumentCache::load(
            &MemHost::seed("/work/.tzlintcache", &file),
            Path::new("/work/.tzlintcache"),
        );
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn cache_load_is_best_effort_on_bad_files_and_entries() {
        let path = Path::new("/work/.tzlintcache");
        let load = |contents: &str| {
            DocumentCache::load(&MemHost::seed("/work/.tzlintcache", contents), path)
        };
        // Whole-file problems → empty cache.
        assert!(DocumentCache::load(&MemHost::new(), path).is_empty()); // missing file
        assert!(load("not json").is_empty()); // corrupt JSON
        assert!(load(r#"{"version":999,"entries":{}}"#).is_empty()); // wrong version
        assert!(load(r#"{"version":1,"entries":["x"]}"#).is_empty()); // `entries` not an object

        // Per-entry problems → that entry is skipped (so the whole cache is empty here).
        let hex = key_of("doc\n", &cfg("{}")).to_hex();
        assert!(load(r#"{"version":1,"entries":{"nothex":[]}}"#).is_empty()); // un-parseable key
        assert!(
            load(&format!(
                r#"{{"version":1,"entries":{{"{hex}":"notarray"}}}}"#
            ))
            .is_empty()
        ); // entry value not an array
        // Diagnostic with an unknown severity → dropped.
        assert!(
            load(&format!(
                r#"{{"version":1,"entries":{{"{hex}":[{{"rule_id":"r","severity":"BOGUS","message":"m","span":{{"start":0,"end":1}}}}]}}}}"#
            ))
            .is_empty()
        );
        // Diagnostic missing a required field → dropped.
        assert!(
            load(&format!(
                r#"{{"version":1,"entries":{{"{hex}":[{{"severity":"error","message":"m","span":{{"start":0,"end":1}}}}]}}}}"#
            ))
            .is_empty()
        );
        // A fix missing its `replacement` → diagnostic dropped.
        assert!(
            load(&format!(
                r#"{{"version":1,"entries":{{"{hex}":[{{"rule_id":"r","severity":"error","message":"m","span":{{"start":0,"end":1}},"fixes":[{{"span":{{"start":0,"end":1}}}}]}}]}}}}"#
            ))
            .is_empty()
        );
        // A present-but-non-array `fixes` → diagnostic dropped (not accepted without its fixes).
        assert!(
            load(&format!(
                r#"{{"version":1,"entries":{{"{hex}":[{{"rule_id":"r","severity":"error","message":"m","span":{{"start":0,"end":1}},"fixes":"notarray"}}]}}}}"#
            ))
            .is_empty()
        );
    }

    #[test]
    fn cache_save_surfaces_write_error() {
        // A host whose write always fails makes `save` return an `Io` error (so a caller can warn).
        struct FailingHost;
        impl Host for FailingHost {
            fn read_to_string(&self, _: &Path, _: usize) -> Result<String, IoError> {
                Err(IoError::NotFound)
            }
            fn write_atomic(&self, _: &Path, _: &[u8]) -> Result<(), IoError> {
                Err(IoError::Other("disk full".into()))
            }
            fn exists(&self, _: &Path) -> bool {
                false
            }
        }
        let result = DocumentCache::new().save(&FailingHost, Path::new("/x/.tzlintcache"));
        assert!(matches!(result, Err(CacheError::Io(_))));
    }

    #[test]
    fn cache_save_rejects_oversize_document() {
        // A document past the read cap must be rejected before writing, not saved as a file the
        // next `load` would silently discard as oversized. `save_within` drives the bound with a
        // tiny limit so no 64 MiB fixture is needed.
        let mut cache = DocumentCache::new();
        cache.insert(
            key_of("doc\n", &cfg("{}")),
            vec![Diagnostic::new(
                "r",
                Severity::Warning,
                Span::new(0, 1),
                "m",
            )],
        );
        let host = MemHost::new();
        let path = Path::new("/work/.tzlintcache");
        let err = cache.save_within(&host, path, 1).unwrap_err();
        assert!(matches!(
            err,
            CacheError::Io(IoError::TooLarge { limit: 1 })
        ));
        // Reject-before-write: nothing was persisted.
        assert!(!host.exists(path));
    }
}
