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
use tzlint_ast::morphology::{
    FeatureKey, Lang, MorphologyBuilder, MorphologyV1, Tagset, Token, TokenAttrs,
};
use tzlint_ast::{NodeId, Span};
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

    /// Shared constructor tail: load the dictionary `uri`, wrap it in a normal-mode segmenter, and
    /// build the immutable tokenizer. Every lindera error becomes a [`MorphologyError::Backend`].
    fn build(uri: &str) -> Result<Self, MorphologyError> {
        let dictionary =
            load_dictionary(uri).map_err(|e| MorphologyError::Backend(e.to_string()))?;
        let segmenter = Segmenter::new(Mode::Normal, dictionary, None);
        Ok(LinderaProvider {
            tokenizer: Tokenizer::new(segmenter),
        })
    }
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

    // The mapping tests need a real tokenizer; they run against lindera's embedded IPADIC, compiled
    // into the test binary by the `embed-ipadic` feature (no network, no committed fixture).
    #[cfg(feature = "embed-ipadic")]
    fn ja() -> LinderaProvider {
        LinderaProvider::with_embedded_ipadic().expect("embedded IPADIC loads")
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
