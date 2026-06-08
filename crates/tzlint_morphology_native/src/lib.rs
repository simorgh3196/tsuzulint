//! Native morphology backend: a [`MorphologyProvider`](tzlint_pdk::MorphologyProvider) backed by
//! the [`lindera`] analyzer.
//!
//! **Native-only.** The whole crate is `#![cfg(not(target_arch = "wasm32"))]`, and — more
//! importantly — no wasm-facing crate (`tzlint_ast`/`tzlint_pdk`/`tzlint_core`) depends on it, so
//! `lindera` is structurally unreachable from the `wasm32` build graph. The engine reaches a real
//! backend only through the `Box<dyn MorphologyProvider>` trait object in
//! `tzlint_core::MorphologyRegistry`, which names no `lindera` type.
#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;

use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;
use lindera::tokenizer::Tokenizer;
use lindera_dictionary::dictionary::Dictionary;
use lindera_dictionary::dictionary::character_definition::CharacterDefinition;
use lindera_dictionary::dictionary::connection_cost_matrix::ConnectionCostMatrix;
use lindera_dictionary::dictionary::metadata::Metadata;
use lindera_dictionary::dictionary::prefix_dictionary::PrefixDictionary;
use lindera_dictionary::dictionary::unknown_dictionary::UnknownDictionary;
use tzlint_ast::morphology::{
    FeatureKey, Lang, MorphologyBuilder, MorphologyV1, Tagset, Token, TokenAttrs,
};
use tzlint_ast::{NodeId, Span};
use tzlint_core::dict::container;
use tzlint_pdk::{MorphologyError, MorphologyProvider};

/// A Japanese [`MorphologyProvider`] backed by lindera + an IPADIC dictionary.
///
/// Construct it over a pre-built dictionary directory with [`new`](LinderaProvider::new), or — for
/// tests — over a dictionary compiled into the binary with
/// [`with_embedded_ipadic`](LinderaProvider::with_embedded_ipadic) (the `embed-ipadic` feature).
/// Both converge on one [`Tokenizer`], so the embedded-dictionary tests exercise the exact
/// production [`analyze`](LinderaProvider::analyze) path.
pub struct LinderaProvider {
    tokenizer: Tokenizer,
}

impl core::fmt::Debug for LinderaProvider {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The lindera tokenizer holds the whole dictionary; it is neither small nor meaningfully
        // printable, so the provider prints opaquely (matching `MorphologyRegistry`'s manual Debug).
        f.debug_struct("LinderaProvider").finish_non_exhaustive()
    }
}

impl LinderaProvider {
    /// Build a provider over the pre-built dictionary **directory** at `dict_dir` (the layout
    /// lindera produces / ships in its releases). A path that does not load — missing, not a
    /// dictionary, non-UTF-8 — is a [`MorphologyError::Backend`], never a panic.
    pub fn new(dict_dir: &Path) -> Result<Self, MorphologyError> {
        let uri = dict_dir.to_str().ok_or_else(|| {
            MorphologyError::Backend(format!(
                "dictionary path is not valid UTF-8: {}",
                dict_dir.display()
            ))
        })?;
        Self::build(uri)
    }

    /// Build a provider over lindera's **embedded** IPADIC (compiled into the binary by the
    /// `embed-ipadic` feature). Test-only in practice: it pulls the whole dictionary into the
    /// binary, so production uses [`new`](LinderaProvider::new) over a directory instead.
    #[cfg(feature = "embed-ipadic")]
    pub fn with_embedded_ipadic() -> Result<Self, MorphologyError> {
        Self::build("embedded://ipadic")
    }

    /// Build a provider from a single in-memory dictionary **container** blob — the decompressed
    /// content of the `.dict.zst` that
    /// [`provision_dictionary`](tzlint_core::provision_dictionary) returns.
    ///
    /// The blob carries the dictionary's component files bundled by
    /// [`tzlint_core::dict::container`]; this splits them back apart **in memory** (no filesystem,
    /// no temp directory) and assembles a lindera [`Dictionary`] from the component byte ranges —
    /// the same shape lindera's own embedded loader uses. It is the representation a host without a
    /// filesystem (a browser/wasm embedder) can also produce, which `new` (a directory path) can
    /// not.
    ///
    /// The container is treated as untrusted: a malformed blob — bad framing, or a connection-cost
    /// matrix whose byte length would make lindera's loader panic — is a [`MorphologyError::Backend`],
    /// never a panic.
    pub fn from_dictionary_bytes(container: &[u8]) -> Result<Self, MorphologyError> {
        let c = container::parse(container)
            .map_err(|e| MorphologyError::Backend(format!("dictionary container: {e}")))?;

        // Guard the connection-cost matrix BEFORE handing it to lindera. `ConnectionCostMatrix::load`
        // computes `len/2 - 3` `i16`s and feeds `&data[6..]` to `byteorder::read_i16_into`, which
        // PANICS unless `src.len() == 2 * dst.len()` — i.e. it panics on an odd-length matrix — and
        // its old-format branch indexes by attacker-declared sizes. Our packaging always emits the
        // "new" transposed format (leading `i16 == -1`, so bytes `FF FF`) with an even length, which
        // takes the panic-free new-format path; require exactly that shape and reject anything else
        // as a corrupt container rather than risk an abort.
        let matrix = c.connection_matrix();
        let new_format_matrix = matrix.len() >= 6
            && matrix.len() % 2 == 0
            && matrix.first() == Some(&0xFF)
            && matrix.get(1) == Some(&0xFF);
        if !new_format_matrix {
            return Err(MorphologyError::Backend(
                "dictionary container: connection matrix is not a valid new-format matrix.mtx"
                    .to_string(),
            ));
        }

        // Assemble the Dictionary exactly as lindera's embedded loader does (component load order,
        // `is_system = true` for a system dictionary). The four prefix-dictionary blobs need owned
        // `Vec<u8>` (lindera's `Data` has no `From<&[u8]>` for a borrowed slice); the rkyv/JSON
        // members take a borrowed `&[u8]` and copy internally.
        let metadata = Metadata::load(c.metadata())
            .map_err(|e| MorphologyError::Backend(format!("dictionary metadata: {e}")))?;
        let prefix_dictionary = PrefixDictionary::load(
            c.prefix_da().to_vec(),
            c.prefix_vals().to_vec(),
            c.prefix_words_idx().to_vec(),
            c.prefix_words().to_vec(),
            // `is_system = true`: a system dictionary. `false` is for user dictionaries and would
            // silently corrupt the value (offset, count) bit-packing.
            true,
        )
        .map_err(|e| MorphologyError::Backend(format!("prefix dictionary: {e}")))?;
        let connection_cost_matrix = ConnectionCostMatrix::load(matrix.to_vec())
            .map_err(|e| MorphologyError::Backend(format!("connection matrix: {e}")))?;
        let character_definition = CharacterDefinition::load(c.char_def())
            .map_err(|e| MorphologyError::Backend(format!("character definition: {e}")))?;
        let unknown_dictionary = UnknownDictionary::load(c.unknown())
            .map_err(|e| MorphologyError::Backend(format!("unknown dictionary: {e}")))?;

        let dictionary = Dictionary {
            prefix_dictionary,
            connection_cost_matrix,
            character_definition,
            unknown_dictionary,
            metadata,
        };
        Ok(Self::from_dictionary(dictionary))
    }

    /// Shared constructor tail: load the dictionary `uri`, then hand off to
    /// [`from_dictionary`](Self::from_dictionary). Every lindera error becomes a
    /// [`MorphologyError::Backend`].
    fn build(uri: &str) -> Result<Self, MorphologyError> {
        let dictionary =
            load_dictionary(uri).map_err(|e| MorphologyError::Backend(e.to_string()))?;
        Ok(Self::from_dictionary(dictionary))
    }

    /// Wrap an already-loaded lindera [`Dictionary`] in a normal-mode segmenter and the immutable
    /// tokenizer. The single convergence point for every constructor — a directory
    /// ([`new`](Self::new)), the embedded IPADIC
    /// ([`with_embedded_ipadic`](Self::with_embedded_ipadic)), and an in-memory container
    /// ([`from_dictionary_bytes`](Self::from_dictionary_bytes)) — so [`analyze`](Self::analyze)
    /// behaves identically no matter how the dictionary was obtained.
    fn from_dictionary(dictionary: Dictionary) -> Self {
        let segmenter = Segmenter::new(Mode::Normal, dictionary, None);
        LinderaProvider {
            tokenizer: Tokenizer::new(segmenter),
        }
    }
}

/// Split a loaded lindera [`Dictionary`] back into the 8 component byte arrays, in
/// [`container::Member`] order — the inverse of the assembly in
/// [`from_dictionary_bytes`](LinderaProvider::from_dictionary_bytes).
///
/// This is the **packaging** side: feed the result to [`container::encode`] to build a
/// distributable container blob (then zstd-compress + hash-pin it). Dev/maintainer-only (behind the
/// `package` feature); the runtime provider never re-serializes a dictionary.
///
/// `dict.vals`/`dict.wordsidx`/`dict.words` round-trip verbatim and `dict.da` is re-`serialize()`d
/// byte-faithfully by daachorse. The two rkyv archives (`char_def.bin`/`unk.bin`) and
/// `metadata.json` are re-serialized to value-equal bytes, and the connection matrix — which lindera
/// keeps only as a parsed `i16` grid — is re-encoded into the "new" `matrix.mtx` format. A
/// load → extract → load round-trip is therefore **value-exact** (proven by the `embed-ipadic`
/// round-trip test), even where the bytes are not identical to the original files.
///
/// # Errors
///
/// [`MorphologyError::Backend`] if re-serializing `metadata.json` or an rkyv archive fails.
#[cfg(feature = "package")]
pub fn extract_components(
    dict: &Dictionary,
) -> Result<[Vec<u8>; container::MEMBER_COUNT], MorphologyError> {
    let metadata = serde_json::to_vec(&dict.metadata)
        .map_err(|e| MorphologyError::Backend(format!("packaging metadata.json: {e}")))?;
    let da = dict.prefix_dictionary.da.serialize();
    let vals = (*dict.prefix_dictionary.vals_data).to_vec();
    let words_idx = (*dict.prefix_dictionary.words_idx_data).to_vec();
    let words = (*dict.prefix_dictionary.words_data).to_vec();
    let matrix = encode_connection_matrix(&dict.connection_cost_matrix);
    let char_def = rkyv::to_bytes::<rkyv::rancor::Error>(&dict.character_definition)
        .map_err(|e| MorphologyError::Backend(format!("packaging char_def.bin: {e}")))?
        .to_vec();
    let unk = rkyv::to_bytes::<rkyv::rancor::Error>(&dict.unknown_dictionary)
        .map_err(|e| MorphologyError::Backend(format!("packaging unk.bin: {e}")))?
        .to_vec();

    let mut out: [Vec<u8>; container::MEMBER_COUNT] = std::array::from_fn(|_| Vec::new());
    out[container::Member::Metadata as usize] = metadata;
    out[container::Member::PrefixDa as usize] = da;
    out[container::Member::PrefixVals as usize] = vals;
    out[container::Member::PrefixWordsIdx as usize] = words_idx;
    out[container::Member::PrefixWords as usize] = words;
    out[container::Member::ConnectionMatrix as usize] = matrix;
    out[container::Member::CharDef as usize] = char_def;
    out[container::Member::Unknown as usize] = unk;
    Ok(out)
}

/// Re-encode lindera's parsed connection-cost grid into a "new"-format `matrix.mtx` byte blob:
/// little-endian `i16` `[-1, forward_size, backward_size]` followed by the cost grid. Always an even
/// length beginning `FF FF` — the shape
/// [`from_dictionary_bytes`](LinderaProvider::from_dictionary_bytes) requires.
#[cfg(feature = "package")]
fn encode_connection_matrix(matrix: &ConnectionCostMatrix) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + matrix.costs_data.len() * 2);
    out.extend_from_slice(&(-1i16).to_le_bytes());
    out.extend_from_slice(&(matrix.forward_size as i16).to_le_bytes());
    out.extend_from_slice(&(matrix.backward_size as i16).to_le_bytes());
    for &cost in &matrix.costs_data {
        out.extend_from_slice(&cost.to_le_bytes());
    }
    out
}

/// The lindera/IPADIC `details()` column index of each canonical [`FeatureKey`]. `base_form`
/// (index 6) and `reading` (index 7) are absent here on purpose: they are promoted to dedicated
/// [`Token`] fields rather than stored as features. Note the asymmetry — `details[8]` (発音) maps
/// to [`FeatureKey::PRONUNCIATION`] (=6).
const FEATURE_COLUMNS: &[(usize, FeatureKey)] = &[
    (0, FeatureKey::POS),
    (1, FeatureKey::POS_SUB_1),
    (2, FeatureKey::POS_SUB_2),
    (3, FeatureKey::POS_SUB_3),
    (4, FeatureKey::CONJUGATION_TYPE),
    (5, FeatureKey::CONJUGATION_FORM),
    (8, FeatureKey::PRONUNCIATION),
];

const COLUMN_BASE_FORM: usize = 6;
const COLUMN_READING: usize = 7;

/// The IPADIC `details()` value at column `i`, or `None` when absent, empty, or the `*` placeholder
/// (so an empty column is never interned as a feature / reading / base_form).
fn column<'a>(details: &[&'a str], i: usize) -> Option<&'a str> {
    details
        .get(i)
        .copied()
        .filter(|value| !value.is_empty() && *value != "*")
}

impl MorphologyProvider for LinderaProvider {
    fn lang(&self) -> Lang {
        Lang::JA
    }

    fn analyze(
        &self,
        text: &str,
        base_offset: u32,
        node: NodeId,
    ) -> Result<MorphologyV1, MorphologyError> {
        // Guard the absolute-offset arithmetic up front (mirrors `WhitespaceProvider`): surfaces are
        // `u32` `Span`s into `Ast::text`, so a node whose text would push past `u32::MAX` is an
        // error, never a panic or an inverted span.
        u32::try_from(text.len())
            .ok()
            .and_then(|n| base_offset.checked_add(n))
            .ok_or_else(|| {
                MorphologyError::Backend(format!(
                    "node text at offset {base_offset} exceeds the u32 address space"
                ))
            })?;

        let mut tokens = self
            .tokenizer
            .tokenize(text)
            .map_err(|e| MorphologyError::Backend(e.to_string()))?;

        let mut builder = MorphologyBuilder::new();
        for token in tokens.iter_mut() {
            // lindera byte offsets are relative to `text`; lift them to document coordinates. The
            // up-front guard proved `base_offset + text.len()` fits, and every token range is within
            // `text`, so these additions cannot overflow — but stay checked rather than asserting.
            let surface = u32::try_from(token.byte_start)
                .ok()
                .and_then(|s| base_offset.checked_add(s))
                .zip(
                    u32::try_from(token.byte_end)
                        .ok()
                        .and_then(|e| base_offset.checked_add(e)),
                )
                .map(|(start, end)| Span::new(start, end))
                .ok_or_else(|| {
                    MorphologyError::Backend(format!(
                        "token byte range {}..{} at offset {base_offset} exceeds the u32 address space",
                        token.byte_start, token.byte_end
                    ))
                })?;

            let flags = if token.word_id.is_unknown() {
                Token::FLAG_UNKNOWN
            } else {
                0
            };

            let details = token.details();
            let features: Vec<(FeatureKey, &str)> = FEATURE_COLUMNS
                .iter()
                .filter_map(|&(index, key)| column(&details, index).map(|value| (key, value)))
                .collect();

            builder.push_token(
                TokenAttrs {
                    node,
                    surface,
                    lang: Lang::JA,
                    tagset: Tagset::IPADIC,
                    flags,
                },
                column(&details, COLUMN_READING),
                column(&details, COLUMN_BASE_FORM),
                &features,
            );
        }
        Ok(builder.finish())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    // `super::*` re-exports the production items plus `NodeId`/`FeatureKey`/`Lang`/`Tagset`/
    // `Token`/`MorphologyError`/`MorphologyProvider` the tests use (a glob, so an item only the
    // embedded-dictionary tests touch never warns when `embed-ipadic` is off).
    use super::*;
    // Archive round-trip helpers — needed only by the embedded-dictionary mapping tests.
    #[cfg(feature = "embed-ipadic")]
    use tzlint_ast::morphology::{access_morphology, to_archive_morphology};

    /// Guards the `Box<dyn MorphologyProvider>` (Send + Sync) bound in the registry: a future
    /// lindera type that lost `Send`/`Sync` would fail HERE at compile time, not at the use site.
    #[test]
    fn provider_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<LinderaProvider>();
    }

    /// The directory constructor's ERROR path needs no real dictionary: a non-existent directory
    /// must be a clean `Err`, never a panic. (Its happy path — a real on-disk dictionary — is
    /// deferred, as it needs a downloaded dictionary, out of the no-network test budget.)
    #[test]
    fn new_with_missing_directory_errors_not_panics() {
        let err =
            LinderaProvider::new(Path::new("/tzlint-nonexistent-dictionary-dir")).unwrap_err();
        assert!(matches!(err, MorphologyError::Backend(_)), "{err}");
    }

    /// Arbitrary bytes are not a dictionary container: a clean `Backend` error, never a panic. (No
    /// real dictionary needed — this fails at the container parse, before any lindera load.)
    #[test]
    fn from_dictionary_bytes_rejects_non_container_bytes() {
        let err = LinderaProvider::from_dictionary_bytes(b"definitely not a dictionary container")
            .unwrap_err();
        assert!(matches!(err, MorphologyError::Backend(_)), "{err}");
    }

    /// A structurally valid container whose connection-matrix member has an odd length — the exact
    /// shape that makes lindera's `ConnectionCostMatrix::load` panic via `read_i16_into`. The
    /// bridge's matrix guard must turn it into a clean `Backend` error, never an abort. The guard
    /// runs before any lindera load, so the other members can be empty placeholders.
    #[test]
    fn from_dictionary_bytes_rejects_a_bad_matrix_without_panicking() {
        let members: [Vec<u8>; container::MEMBER_COUNT] = std::array::from_fn(|i| {
            if i == container::Member::ConnectionMatrix as usize {
                vec![0xFF, 0xFF, 0x00] // odd length: would panic read_i16_into without the guard
            } else {
                Vec::new()
            }
        });
        let refs: [&[u8]; container::MEMBER_COUNT] = std::array::from_fn(|i| members[i].as_slice());
        let blob = container::encode(&refs).expect("encodes");
        let err = LinderaProvider::from_dictionary_bytes(&blob).unwrap_err();
        assert!(matches!(err, MorphologyError::Backend(_)), "{err}");
        assert!(err.to_string().contains("connection matrix"), "{err}");
    }

    // The mapping tests need a real tokenizer; they run against lindera's embedded IPADIC, compiled
    // into the test binary by the `embed-ipadic` feature (no network, no committed fixture).
    #[cfg(feature = "embed-ipadic")]
    fn ja() -> LinderaProvider {
        LinderaProvider::with_embedded_ipadic().expect("embedded IPADIC loads")
    }

    /// The headline proof that the in-memory container bridge WORKS on a real dictionary: pack the
    /// embedded IPADIC into a container, rehydrate a provider from it via `from_dictionary_bytes`,
    /// and assert it tokenizes real Japanese **byte-for-byte identically** to the embedded reference.
    /// Equal token streams (surfaces, features, readings) prove the round-trip is value-exact —
    /// including the re-encoded connection-cost matrix and the `is_system = true` bit-packing, the
    /// two places a packaging bug would silently corrupt segmentation.
    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn from_dictionary_bytes_round_trips_embedded_ipadic_and_tokenizes_identically() {
        let dict = load_dictionary("embedded://ipadic").expect("embedded IPADIC loads");
        let components = extract_components(&dict).expect("extract components");
        let refs: [&[u8]; container::MEMBER_COUNT] =
            std::array::from_fn(|i| components[i].as_slice());
        let blob = container::encode(&refs).expect("encode container");

        let rehydrated =
            LinderaProvider::from_dictionary_bytes(&blob).expect("rehydrate from container");
        let reference = ja();

        for text in [
            "関西国際空港限定トートバッグ",
            "すもももももももものうち",
            "本を読む",
            "私は彼は来た",
        ] {
            let from_container = rehydrated
                .analyze(text, 0, NodeId(0))
                .expect("rehydrated analyze");
            let from_embedded = reference
                .analyze(text, 0, NodeId(0))
                .expect("reference analyze");
            assert!(!from_container.tokens.is_empty(), "no tokens for {text:?}");
            // Byte-for-byte equality of the whole MorphologyV1 (tokens + features + interned
            // strings) — the strongest equivalence, and it localizes any matrix/packing regression.
            let a = to_archive_morphology(&from_container).expect("archive rehydrated");
            let b = to_archive_morphology(&from_embedded).expect("archive reference");
            // `AlignedVec` has no `PartialEq`; compare the archived bytes directly (Deref to [u8]).
            assert_eq!(&a[..], &b[..], "token streams differ for {text:?}");
        }
    }

    /// The matrix guard's panic-prevention, on a REAL dictionary: valid metadata/prefix members but
    /// a connection-matrix member truncated to an odd length — the exact shape that makes lindera's
    /// `ConnectionCostMatrix::load` panic in `read_i16_into`. The guard must turn it into a clean
    /// `Backend` error. (Removing the guard makes this test abort instead of asserting, so it pins
    /// the guard's necessity against the real loader.)
    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn from_dictionary_bytes_rejects_a_corrupt_real_matrix_without_panicking() {
        let dict = load_dictionary("embedded://ipadic").expect("embedded IPADIC loads");
        let mut components = extract_components(&dict).expect("extract components");
        components[container::Member::ConnectionMatrix as usize].pop(); // → odd length
        let refs: [&[u8]; container::MEMBER_COUNT] =
            std::array::from_fn(|i| components[i].as_slice());
        let blob = container::encode(&refs).expect("encode container");
        let err = LinderaProvider::from_dictionary_bytes(&blob).unwrap_err();
        assert!(matches!(err, MorphologyError::Backend(_)), "{err}");
        assert!(err.to_string().contains("connection matrix"), "{err}");
    }

    /// The end-to-end firing proof through the engine: a REAL lindera provider, wired into a
    /// [`MorphologyRegistry`] and run through `tzlint_core::lint_document`, makes the
    /// `no-doubled-joshi` morphology rule FIRE on real Japanese — and a run with no provider leaves
    /// it inert. This closes the loop the rest of the suite proves piecewise (the bridge tokenizes;
    /// the analysis pass builds the table; the rule reads it).
    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn no_doubled_joshi_fires_through_lint_document_with_a_real_provider() {
        use tzlint_core::{
            DictId, MorphologyRegistry, ProcessorConfig, RegionRules, Registry, lint_document,
        };
        use tzlint_rules::NoDoubledJoshi;

        // A paragraph repeating the particle は in one sentence — the canonical doubled-joshi case.
        let source = "私は彼は本を読む。\n";
        let rules = RegionRules::base_only(vec![Box::new(NoDoubledJoshi::default())]);
        let fires = |morphology: Option<&MorphologyRegistry>| {
            lint_document(
                Some("md"),
                source,
                &Registry::with_builtins(),
                &ProcessorConfig::default(),
                &rules,
                morphology,
            )
            .expect("lint_document")
            .iter()
            .any(|d| d.rule_id.as_str() == "no-doubled-joshi")
        };

        let mut reg = MorphologyRegistry::new();
        reg.insert(Box::new(ja()), DictId::from_pin([7; 32]));
        assert!(fires(Some(&reg)), "rule must fire with a real JA provider");
        assert!(
            !fires(None),
            "rule must stay inert without a morphology provider"
        );
    }

    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn provider_reports_japanese_and_prints_opaquely() {
        let provider = ja();
        assert_eq!(provider.lang(), Lang::JA);
        // Debug omits the dictionary-bearing tokenizer (it just names the type).
        assert!(format!("{provider:?}").contains("LinderaProvider"));
    }

    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn tokenizes_japanese_into_absolute_spans() {
        let table = ja().analyze("東京", 0, NodeId(0)).unwrap();
        assert!(!table.tokens.is_empty(), "IPADIC tokenizes 東京");
        // "東京" is 6 bytes; every surface is an absolute byte span inside 0..6.
        for t in &table.tokens {
            assert_eq!(t.lang, Lang::JA);
            assert_eq!(t.tagset, Tagset::IPADIC);
            assert_eq!(t.node, NodeId(0));
            assert!(t.surface.start < t.surface.end);
            assert!(t.surface.end <= 6, "surface {:?} within 0..6", t.surface);
        }
        // A dictionary-backed token carries a POS feature.
        let bytes = to_archive_morphology(&table).unwrap();
        let archived = access_morphology(&bytes).unwrap();
        let first = &archived.tokens()[0];
        assert!(
            first
                .features(archived)
                .any(|(k, v)| k == FeatureKey::POS && v.is_some()),
            "first token has a POS feature"
        );
    }

    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn base_offset_and_multibyte_shift_surfaces() {
        // Every CJK char is 3 bytes; surfaces must be base_offset + lindera byte range.
        let base = 10;
        let table = ja().analyze("日本語の文", base, NodeId(3)).unwrap();
        assert!(!table.tokens.is_empty());
        for t in &table.tokens {
            assert_eq!(t.node, NodeId(3));
            assert!(t.surface.start >= base, "shifted by base_offset");
            assert!(t.surface.end <= base + 15, "5 CJK chars = 15 bytes");
        }
        // Contiguous coverage: first starts at base, last ends at base+15.
        assert_eq!(table.tokens.first().unwrap().surface.start, base);
        assert_eq!(table.tokens.last().unwrap().surface.end, base + 15);
    }

    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn reading_and_base_form_are_promoted_to_distinct_fields() {
        // details[6]=base_form and details[7]=reading become dedicated Token fields, never features —
        // and they are DISTINCT, so a column swap is caught. 東京 (proper noun) is a clean probe:
        // base_form 東京 (== surface) vs reading トウキョウ (katakana). Asserting the surface-equal
        // base form is IPADIC-version-stable; the distinctness kills the details[6]<->[7] swap.
        let table = ja().analyze("東京", 0, NodeId(0)).unwrap();
        let bytes = to_archive_morphology(&table).unwrap();
        let archived = access_morphology(&bytes).unwrap();
        let noun = archived
            .tokens()
            .iter()
            .find(|t| {
                t.features(archived)
                    .any(|(k, v)| k == FeatureKey::POS && v == Some("名詞"))
            })
            .expect("a 名詞 token for 東京");
        assert_eq!(
            noun.base_form(archived),
            Some("東京"),
            "base_form is details[6] (== surface for 東京), not the reading"
        );
        let reading = noun.reading(archived).expect("reading promoted");
        assert_ne!(
            Some(reading),
            noun.base_form(archived),
            "reading (details[7]) is distinct from base_form (details[6])"
        );
        // FeatureKey has no reading/base_form member, so every feature key is a known mapping key —
        // i.e. neither column 6 nor 7 leaked into the feature store.
        for (k, _) in noun.features(archived) {
            assert!(
                [
                    FeatureKey::POS,
                    FeatureKey::POS_SUB_1,
                    FeatureKey::POS_SUB_2,
                    FeatureKey::POS_SUB_3,
                    FeatureKey::CONJUGATION_TYPE,
                    FeatureKey::CONJUGATION_FORM,
                    FeatureKey::PRONUNCIATION,
                ]
                .contains(&k),
                "feature key {k:?} is a canonical IPADIC column"
            );
        }
    }

    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn pronunciation_maps_details_index8_not_index7() {
        // The asymmetric mapping: details[8] (発音) -> FeatureKey::PRONUNCIATION (=6), NOT details[7]
        // (読み). 東京 discriminates them: reading(7) トウキョウ vs 発音(8) トーキョー. If PRONUNCIATION
        // were sourced from details[7], its value would equal the reading — so asserting they DIFFER
        // kills the index-7/8 confusion (which 行う could not catch — its 読み == 発音).
        let table = ja().analyze("東京", 0, NodeId(0)).unwrap();
        let bytes = to_archive_morphology(&table).unwrap();
        let archived = access_morphology(&bytes).unwrap();
        let noun = archived
            .tokens()
            .iter()
            .find(|t| {
                t.features(archived)
                    .any(|(k, v)| k == FeatureKey::POS && v == Some("名詞"))
            })
            .expect("a 名詞 token");
        let pron = noun
            .features(archived)
            .find(|(k, _)| *k == FeatureKey::PRONUNCIATION)
            .and_then(|(_, v)| v)
            .expect("pronunciation present at FeatureKey 6");
        assert_ne!(
            Some(pron),
            noun.reading(archived),
            "PRONUNCIATION comes from details[8] (発音), distinct from the details[7] reading"
        );
    }

    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn conjugation_columns_map_for_a_verb() {
        // Pins the details[4]->CONJUGATION_TYPE and details[5]->CONJUGATION_FORM mapping with a
        // conjugating verb (行う), whose 活用型/活用形 columns are populated (not '*').
        let table = ja().analyze("行う", 0, NodeId(0)).unwrap();
        let bytes = to_archive_morphology(&table).unwrap();
        let archived = access_morphology(&bytes).unwrap();
        let verb = archived
            .tokens()
            .iter()
            .find(|t| {
                t.features(archived)
                    .any(|(k, v)| k == FeatureKey::POS && v == Some("動詞"))
            })
            .expect("a 動詞 token for 行う");
        let conj_type = verb
            .features(archived)
            .find(|(k, _)| *k == FeatureKey::CONJUGATION_TYPE)
            .and_then(|(_, v)| v);
        let conj_form = verb
            .features(archived)
            .find(|(k, _)| *k == FeatureKey::CONJUGATION_FORM)
            .and_then(|(_, v)| v);
        assert_eq!(
            conj_type,
            Some("五段・ワ行促音便"),
            "details[4] -> CONJUGATION_TYPE"
        );
        assert_eq!(conj_form, Some("基本形"), "details[5] -> CONJUGATION_FORM");
    }

    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn empty_and_blank_inputs_yield_no_tokens() {
        // The up-front length guard and an empty tokenize result must produce a valid, empty table —
        // no spurious token, no error (mirrors WhitespaceProvider's empty/blank test).
        assert!(ja().analyze("", 0, NodeId(0)).unwrap().tokens.is_empty());
        assert!(ja().analyze("   ", 0, NodeId(0)).unwrap().tokens.is_empty());
    }

    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn star_placeholder_columns_are_omitted() {
        // 東京 is a proper noun: its 活用型/活用形 columns are "*", which must produce NO feature.
        let table = ja().analyze("東京", 0, NodeId(0)).unwrap();
        let bytes = to_archive_morphology(&table).unwrap();
        let archived = access_morphology(&bytes).unwrap();
        for t in archived.tokens() {
            for (_, v) in t.features(archived) {
                assert_ne!(v, Some("*"), "a '*' column must be dropped, never interned");
            }
            // A noun does not conjugate: no CONJUGATION_* feature survives the '*' omission.
            assert!(
                !t.features(archived)
                    .any(|(k, _)| k == FeatureKey::CONJUGATION_TYPE
                        || k == FeatureKey::CONJUGATION_FORM),
                "noun has no conjugation columns"
            );
        }
    }

    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn unknown_words_set_flag_unknown() {
        // A run lindera cannot find in IPADIC is marked unknown; the canonical OOV bit must be set.
        let table = ja().analyze("qwzx", 0, NodeId(0)).unwrap();
        assert!(
            table
                .tokens
                .iter()
                .any(|t| t.flags & Token::FLAG_UNKNOWN != 0),
            "an out-of-dictionary run sets FLAG_UNKNOWN"
        );
    }

    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn offset_overflow_is_a_backend_error_not_a_panic() {
        let err = ja().analyze("あ", u32::MAX - 1, NodeId(0)).unwrap_err();
        assert!(matches!(err, MorphologyError::Backend(_)), "{err}");
    }

    #[cfg(feature = "embed-ipadic")]
    #[test]
    fn output_archives_and_reads_back() {
        // The produced table is a valid frozen MorphologyV1: archive and read via the checked path.
        let table = ja().analyze("日本語", 0, NodeId(7)).unwrap();
        let bytes = to_archive_morphology(&table).unwrap();
        let archived = access_morphology(&bytes).unwrap();
        assert!(archived.tokens_of(NodeId(7)).count() >= 1);
        let first = &archived.tokens()[0];
        assert_eq!(first.surface().start, 0);
        assert!(first.features(archived).count() >= 1);
    }
}
