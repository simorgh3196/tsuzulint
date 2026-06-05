//! Per-run morphology provider registry (M2h) + the dictionary cache fingerprint (M2e).
//!
//! A [`MorphologyRegistry`] is the seam through which an embedder injects morphology providers
//! (Japanese first), each paired with a [`DictId`] — the identity of the dictionary it analyzes
//! with. `tzlint_core` ships no providers, so an unconfigured run holds an empty registry and is
//! byte-for-byte identical to the pre-morphology cache key.
//!
//! This module's one job today is the **cache fingerprint**: [`MorphologyRegistry::fingerprint`]
//! folds the *active* dictionaries (registered ∩ required-by-the-enabled-rules) into the
//! `morphology_fingerprint` slot of the document cache key, so a dictionary upgrade invalidates
//! stale cached diagnostics. Running providers over a document to build a
//! [`MorphologyV1`](tzlint_ast::morphology::MorphologyV1) and feeding `Engine::lint` is the
//! **analysis pass**, deferred to M2l (the first rule that reads tokens); the engine is untouched
//! here and each registered `provider` is stored but only its [`lang`](MorphologyProvider::lang)
//! is read this slice. The registry never touches [`Host`](crate::Host), `dict`, or the network,
//! so it stays `wasm32`-clean and dictionary-free by default.

use core::fmt;

use blake3::Hasher;

use tzlint_ast::morphology::{Lang, MORPHOLOGY_INTERFACE_VERSION};
use tzlint_pdk::MorphologyProvider;

use crate::RegionRules;

/// BLAKE3 derive-key domain for the morphology fingerprint — distinct from `cache.rs`'s
/// `KEY_CONTEXT`/`CONFIG_CONTEXT`, so the hash families can never alias. The `v1` suffix is the
/// single source of truth for this encoding's identity (no separate version const).
const FINGERPRINT_CONTEXT: &str = "tsuzulint morphology fingerprint v1";

/// A dictionary identity: the BLAKE3 pin over the **compressed** dictionary blob — exactly the
/// value [`provision_dictionary`](crate::provision_dictionary) verifies and hash-addresses. Folded
/// into the cache fingerprint so a dictionary upgrade (which changes the pin) changes the key.
/// Re-compressing an otherwise-identical dictionary changes the pin and churns active keys (a
/// spurious recompute, never unsafe).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DictId([u8; 32]);

impl DictId {
    /// Wrap the pinned BLAKE3 hash the host already passes to
    /// [`provision_dictionary`](crate::provision_dictionary) (a zero-cost handoff).
    #[must_use]
    pub const fn from_pin(pin: [u8; 32]) -> Self {
        DictId(pin)
    }

    /// The raw 32 identity bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// One registered language: the provider that analyzes it (run by the deferred M2l analysis pass;
/// only [`lang`](MorphologyProvider::lang) is read this slice) and the dictionary identity folded
/// into the fingerprint.
struct MorphologyEntry {
    // Stored now so M2l's analysis pass reads the provider for exactly this language without a
    // second parallel map; only `lang()` is consulted this slice (at `insert`), so the field itself
    // is not yet read. `expect` (not `allow`) so this trips once M2l uses it — a prompt to drop it.
    #[expect(
        dead_code,
        reason = "read by M2l's analysis pass; only lang() is used this slice"
    )]
    provider: Box<dyn MorphologyProvider>,
    dict_id: DictId,
}

/// Per-run morphology providers, injected Host-style by the embedder before linting.
///
/// `tzlint_core` ships no default providers, so an empty registry (the [`Default`]) produces an
/// empty fingerprint and a byte-identical cache key.
#[derive(Default)]
pub struct MorphologyRegistry {
    /// Sorted by `Lang::as_u32()`, one entry per `Lang` (last insert wins).
    entries: Vec<(Lang, MorphologyEntry)>,
}

impl MorphologyRegistry {
    /// An empty registry — equivalent to no morphology (an empty fingerprint, a pre-M2 cache key).
    #[must_use]
    pub fn new() -> Self {
        MorphologyRegistry {
            entries: Vec::new(),
        }
    }

    /// Register `provider` (keyed by its own [`lang`](MorphologyProvider::lang)) with dictionary
    /// identity `dict_id`, replacing any existing entry for that language (last insert wins). The
    /// key comes from the provider itself, so a key/value mismatch is unrepresentable.
    pub fn insert(&mut self, provider: Box<dyn MorphologyProvider>, dict_id: DictId) {
        let lang = provider.lang();
        let entry = MorphologyEntry { provider, dict_id };
        match self
            .entries
            .binary_search_by_key(&lang.as_u32(), |(l, _)| l.as_u32())
        {
            Ok(pos) => self.entries[pos] = (lang, entry),
            Err(pos) => self.entries.insert(pos, (lang, entry)),
        }
    }

    /// The cache-key fingerprint of the dictionaries active for `rules`: the intersection of the
    /// registered languages and the languages `rules` require. Returns an **empty `Vec`** when that
    /// intersection is empty (no providers, providers for unneeded languages, or needed languages
    /// with no provider), which `document_cache_key` folds as "kind = none", keeping a byte-
    /// identical cache key. Otherwise a 32-byte BLAKE3 digest.
    ///
    /// The interface version is folded **inside** the non-empty branch, so a
    /// [`MORPHOLOGY_INTERFACE_VERSION`] bump invalidates keys only for morphology-active documents
    /// (an inactive document has no morphology behavior to invalidate, and keeps its pre-M2 key).
    #[must_use]
    pub fn fingerprint(&self, rules: &RegionRules) -> Vec<u8> {
        let required = rules.required_langs();
        let mut active: Vec<(Lang, &DictId)> = self
            .entries
            .iter()
            .filter(|(lang, _)| required.iter().any(|r| r == lang))
            .map(|(lang, entry)| (*lang, &entry.dict_id))
            .collect();
        if active.is_empty() {
            return Vec::new();
        }
        // Deterministic, order-independent: defensively re-sort (the `entries` invariant already
        // yields this order) so the digest never depends on `required_langs()` ordering.
        active.sort_by_key(|(lang, _)| lang.as_u32());

        let mut hasher = Hasher::new_derive_key(FINGERPRINT_CONTEXT);
        hasher.update(&MORPHOLOGY_INTERFACE_VERSION.to_le_bytes());
        hasher.update(&(active.len() as u64).to_le_bytes());
        for (lang, dict_id) in active {
            hasher.update(&lang.as_u32().to_le_bytes());
            put(&mut hasher, dict_id.as_bytes());
        }
        hasher.finalize().as_bytes().to_vec()
    }
}

impl fmt::Debug for MorphologyRegistry {
    /// Lists the registered `(Lang, DictId)` pairs; the providers are opaque (`MorphologyProvider`
    /// is not `Debug`), so they are omitted.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MorphologyRegistry")
            .field(
                "entries",
                &self
                    .entries
                    .iter()
                    .map(|(lang, entry)| (lang.as_u32(), entry.dict_id))
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// Length-prefix `field` (u64-LE) then append it — the same discipline as `cache::put`, so a
/// distinct sequence of dict ids cannot collide by re-segmentation.
fn put(hasher: &mut Hasher, field: &[u8]) {
    hasher.update(&(field.len() as u64).to_le_bytes());
    hasher.update(field);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RegionRules;
    use tzlint_ast::NodeKind;
    use tzlint_pdk::{Context, NodeRef, Rule, RuleMeta, Severity, WhitespaceProvider};

    /// A rule whose only purpose is to declare a morphology requirement for a language.
    struct MorphRule(RuleMeta);
    impl Rule for MorphRule {
        fn meta(&self) -> &RuleMeta {
            &self.0
        }
        fn check<'a>(&self, _n: NodeRef<'a>, _cx: &mut Context<'a>) {}
    }
    fn morph_rule(id: &str, lang: Lang) -> Box<dyn Rule> {
        Box::new(MorphRule(
            RuleMeta::new(id, Severity::Warning, vec![NodeKind::TEXT]).with_morphology(lang),
        ))
    }
    fn rules_needing(langs: &[Lang]) -> RegionRules {
        let base = langs
            .iter()
            .enumerate()
            .map(|(i, l)| morph_rule(&format!("r{i}"), *l))
            .collect();
        RegionRules::base_only(base)
    }
    fn provider(lang: Lang) -> Box<dyn MorphologyProvider> {
        Box::new(WhitespaceProvider::new(lang))
    }

    // Test 1: empty registry → empty fingerprint, even when a rule needs morphology.
    #[test]
    fn empty_registry_yields_empty_fingerprint() {
        let reg = MorphologyRegistry::new();
        assert!(reg.fingerprint(&rules_needing(&[Lang::JA])).is_empty());
        assert!(reg.fingerprint(&RegionRules::base_only(vec![])).is_empty());
    }

    // Debug lists the (lang, dict_id) pairs and never leaks the opaque provider; Default == new().
    #[test]
    fn registry_debug_lists_entries_and_default_is_empty() {
        let mut reg = MorphologyRegistry::new();
        reg.insert(provider(Lang::JA), DictId::from_pin([0x55; 32])); // 0x55 == 85
        let s = format!("{reg:?}");
        assert!(s.contains("MorphologyRegistry"));
        assert!(s.contains("entries"));
        assert!(s.contains("DictId"));
        assert!(s.contains("85"), "the dict id bytes must appear: {s}");
        // `Default` builds the same empty registry as `new()` (empty fingerprint, debuggable).
        let default = MorphologyRegistry::default();
        assert!(default.fingerprint(&rules_needing(&[Lang::JA])).is_empty());
        assert!(format!("{default:?}").contains("MorphologyRegistry"));
    }

    // Test 4: a registered + required dictionary → 32 non-empty bytes.
    #[test]
    fn active_dictionary_yields_a_32_byte_fingerprint() {
        let mut reg = MorphologyRegistry::new();
        reg.insert(provider(Lang::JA), DictId::from_pin([0xAB; 32]));
        let fp = reg.fingerprint(&rules_needing(&[Lang::JA]));
        assert_eq!(fp.len(), 32);
    }

    // Test 5: dictionary-version addressing — a different DictId → a different fingerprint.
    #[test]
    fn different_dict_id_changes_the_fingerprint() {
        let fp = |pin: [u8; 32]| {
            let mut reg = MorphologyRegistry::new();
            reg.insert(provider(Lang::JA), DictId::from_pin(pin));
            reg.fingerprint(&rules_needing(&[Lang::JA]))
        };
        assert_ne!(fp([0xAB; 32]), fp([0xCD; 32]));
    }

    // Test 6: active-set intersection (both directions) → empty.
    #[test]
    fn intersection_is_empty_in_both_mismatch_directions() {
        // Provider for JA, but only a KO-needing rule.
        let mut ja_only = MorphologyRegistry::new();
        ja_only.insert(provider(Lang::JA), DictId::from_pin([1; 32]));
        assert!(ja_only.fingerprint(&rules_needing(&[Lang::KO])).is_empty());
        // Provider for JA, but a no-morphology rule set.
        assert!(
            ja_only
                .fingerprint(&RegionRules::base_only(vec![]))
                .is_empty()
        );
        // Rule needs JA, but only a KO provider registered.
        let mut ko_only = MorphologyRegistry::new();
        ko_only.insert(provider(Lang::KO), DictId::from_pin([2; 32]));
        assert!(ko_only.fingerprint(&rules_needing(&[Lang::JA])).is_empty());
    }

    // Test 7: determinism + order-invariance across insert order.
    #[test]
    fn fingerprint_is_insert_order_independent() {
        // Same (Lang -> DictId) mapping, different insert order → identical fingerprint, because
        // `insert` keys by `provider.lang()`: JA always gets 0x11, KO always gets 0x22.
        let ja_first = {
            let mut reg = MorphologyRegistry::new();
            reg.insert(provider(Lang::JA), DictId::from_pin([0x11; 32]));
            reg.insert(provider(Lang::KO), DictId::from_pin([0x22; 32]));
            reg.fingerprint(&rules_needing(&[Lang::JA, Lang::KO]))
        };
        let ko_first = {
            let mut reg = MorphologyRegistry::new();
            reg.insert(provider(Lang::KO), DictId::from_pin([0x22; 32]));
            reg.insert(provider(Lang::JA), DictId::from_pin([0x11; 32]));
            reg.fingerprint(&rules_needing(&[Lang::JA, Lang::KO]))
        };
        assert_eq!(ja_first, ko_first);
        assert!(!ja_first.is_empty());
    }

    // Test 9: last-wins insert for the same language.
    #[test]
    fn last_insert_wins_for_a_language() {
        let r = rules_needing(&[Lang::JA]);
        let mut first_then_second = MorphologyRegistry::new();
        first_then_second.insert(provider(Lang::JA), DictId::from_pin([0xAA; 32]));
        first_then_second.insert(provider(Lang::JA), DictId::from_pin([0xBB; 32]));
        // A registry with only the second DictId must produce the same fingerprint.
        let mut second_only = MorphologyRegistry::new();
        second_only.insert(provider(Lang::JA), DictId::from_pin([0xBB; 32]));
        assert_eq!(
            first_then_second.fingerprint(&r),
            second_only.fingerprint(&r)
        );
        // And different from the first DictId alone.
        let mut first_only = MorphologyRegistry::new();
        first_only.insert(provider(Lang::JA), DictId::from_pin([0xAA; 32]));
        assert_ne!(
            first_then_second.fingerprint(&r),
            first_only.fingerprint(&r)
        );
    }

    // Test 10: the interface version is in the hash (differential vs a hypothetical bump).
    #[test]
    fn interface_version_is_folded_into_the_fingerprint() {
        // Reproduce the production byte sequence for one active (JA, [0x33;32]) entry, then prove a
        // different interface version yields a different digest — without asserting an exact golden.
        let fold = |version: u32| {
            let mut h = Hasher::new_derive_key(FINGERPRINT_CONTEXT);
            h.update(&version.to_le_bytes());
            h.update(&1u64.to_le_bytes());
            h.update(&Lang::JA.as_u32().to_le_bytes());
            put(&mut h, &[0x33; 32]);
            h.finalize().as_bytes().to_vec()
        };
        let mut reg = MorphologyRegistry::new();
        reg.insert(provider(Lang::JA), DictId::from_pin([0x33; 32]));
        let produced = reg.fingerprint(&rules_needing(&[Lang::JA]));
        assert_eq!(produced, fold(MORPHOLOGY_INTERFACE_VERSION));
        // A hypothetical V2 differs → the version genuinely participates.
        assert_ne!(
            fold(MORPHOLOGY_INTERFACE_VERSION),
            fold(MORPHOLOGY_INTERFACE_VERSION + 1)
        );
    }
}
