//! Per-run morphology provider registry (M2h) + the dictionary cache fingerprint (M2e).
//!
//! A [`MorphologyRegistry`] is the seam through which an embedder injects morphology providers
//! (Japanese first), each paired with a [`DictId`] — the identity of the dictionary it analyzes
//! with. `tzlint_core` ships no providers, so an unconfigured run holds an empty registry and is
//! byte-for-byte identical to the pre-morphology cache key.
//!
//! This module does two things. (1) The **cache fingerprint**:
//! [`MorphologyRegistry::fingerprint`] folds the *active* dictionaries (registered ∩
//! required-by-the-enabled-rules) into the `morphology_fingerprint` slot of the document cache key,
//! so a dictionary upgrade invalidates stale cached diagnostics. (2) The **analysis pass**:
//! [`MorphologyRegistry::build_table`] runs the active providers over a document's archived AST and
//! merges their tokens into one [`MorphologyV1`](tzlint_ast::morphology::MorphologyV1) for
//! `Engine::lint` (wired in at `lint_document`), keyed to the nodes the morphology rules visit. A
//! real tokenizing backend (lindera, M2j) and the rule that reads the tokens (`no-doubled-joshi`,
//! M2l) are still to come; the dictionary-free [`WhitespaceProvider`] exercises the seam
//! end-to-end today. The registry never touches [`Host`](crate::Host), `dict`, or the network, so
//! it stays `wasm32`-clean and dictionary-free by default.

use core::fmt;

use blake3::Hasher;

use tzlint_ast::morphology::{
    FeatureKey, Lang, MORPHOLOGY_INTERFACE_VERSION, MorphologyBuilder, MorphologyV1, TokenAttrs,
};
use tzlint_ast::{ArchivedAst, NodeKind};
use tzlint_pdk::{MorphologyError, MorphologyProvider, NodeRef, Rule};

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
    // The provider that tokenizes this language; read by [`MorphologyRegistry::active_providers`]
    // (the analysis pass runs it over the document) and keyed by its own `lang()` at `insert`.
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

    /// The providers **active** for `rules`: the registered languages that some rule in `rules`
    /// requires (`required_lang()`), each with its provider, in `Lang::as_u32()` order. This is the
    /// analysis-pass counterpart of [`fingerprint`](Self::fingerprint)'s active set, but keyed off
    /// the already-`for_tag`-resolved rule slice the engine will run (not a whole `RegionRules`).
    /// Empty when no enabled rule needs a registered language — the analysis pass then builds no
    /// table (`Engine::lint` runs with `None`, exactly as before morphology).
    pub(crate) fn active_providers<'a>(
        &'a self,
        rules: &[&dyn Rule],
    ) -> Vec<(Lang, &'a dyn MorphologyProvider)> {
        let mut required: Vec<Lang> = rules
            .iter()
            .filter_map(|r| r.meta().required_lang())
            .collect();
        required.sort_by_key(|l| l.as_u32());
        required.dedup();
        self.entries
            .iter()
            .filter(|(lang, _)| {
                required
                    .binary_search_by_key(&lang.as_u32(), |l| l.as_u32())
                    .is_ok()
            })
            .map(|(lang, entry)| (*lang, entry.provider.as_ref()))
            .collect()
    }

    /// Run the active providers over `archived` and merge their per-node tokens into one
    /// [`MorphologyV1`] for `Engine::lint`. Returns `Ok(None)` when no provider is active for
    /// `rules` (the analysis pass builds no table and the engine runs with `None`, exactly as
    /// before morphology). Tokens are keyed to the nodes whose kind the active morphology rules
    /// visit (today `TEXT`) — so `token NodeId == visited NodeId` and `cx.tokens_of(node.id())`
    /// returns that node's tokens. Each matching node is tokenized in place at its absolute
    /// `node.span().start`, so every surface stays an absolute `Span` into `Ast::text`.
    ///
    /// A provider failure propagates as [`MorphologyError`] (the caller maps it to a parse-style
    /// error and fails the region's lint) rather than silently dropping a node.
    pub(crate) fn build_table(
        &self,
        archived: &ArchivedAst,
        rules: &[&dyn Rule],
    ) -> Result<Option<MorphologyV1>, MorphologyError> {
        let providers = self.active_providers(rules);
        if providers.is_empty() {
            return Ok(None);
        }
        // The node kinds the active morphology rules visit — tokens key to exactly these nodes.
        let active_kinds: Vec<NodeKind> = rules
            .iter()
            .filter(|rule| rule.meta().needs_morphology())
            .flat_map(|rule| rule.meta().node_kinds.iter().copied())
            .collect();

        // Collect matching nodes via a cycle-safe pre-order walk (mirrors `Engine::lint`), then sort
        // node-ascending so tokens land in the documented `(node, surface.start)` order.
        let mut nodes: Vec<NodeRef> = Vec::new();
        if let Some(root) = NodeRef::root(archived) {
            let mut visited = vec![false; archived.len()];
            if let Some(slot) = visited.get_mut(root.id().0 as usize) {
                *slot = true;
            }
            let mut stack = vec![root];
            while let Some(node) = stack.pop() {
                if active_kinds.contains(&node.kind()) {
                    nodes.push(node);
                }
                for child in node.children() {
                    if let Some(slot) = visited.get_mut(child.id().0 as usize)
                        && !*slot
                    {
                        *slot = true;
                        stack.push(child);
                    }
                }
            }
        }
        nodes.sort_by_key(|node| node.id().0);

        let mut builder = MorphologyBuilder::new();
        for node in nodes {
            let text = node.text();
            let base = node.span().start;

            if providers.len() == 1 {
                // Fast path: only one provider active (the common case). No temp sorting buffer.
                let (_lang, provider) = &providers[0];
                let table = provider.analyze(text, base, node.id())?;
                let pool = table.strings.as_str();
                for token in &table.tokens {
                    let start = (token.features_start as usize).min(table.features.len());
                    let end = start
                        .saturating_add(token.features_len as usize)
                        .min(table.features.len());
                    let features: Vec<(FeatureKey, &str)> = table.features[start..end]
                        .iter()
                        .filter_map(|feature| {
                            feature.value.get(pool).map(|value| (feature.key, value))
                        })
                        .collect();
                    builder.push_token(
                        TokenAttrs {
                            node: token.node,
                            surface: token.surface,
                            lang: token.lang,
                            tagset: token.tagset,
                            flags: token.flags,
                        },
                        token.reading.get(pool),
                        token.base_form.get(pool),
                        &features,
                    );
                }
            } else {
                // Slow path: multiple providers are active (e.g. multi-lingual region).
                // Collect all tokens, sort them by surface.start to preserve the documented
                // (node, surface.start) order, and then merge.
                let mut tables = Vec::new();
                for (_lang, provider) in &providers {
                    tables.push(provider.analyze(text, base, node.id())?);
                }

                let mut temp_tokens = Vec::new();
                for table in &tables {
                    let pool = table.strings.as_str();
                    for token in &table.tokens {
                        let start = (token.features_start as usize).min(table.features.len());
                        let end = start
                            .saturating_add(token.features_len as usize)
                            .min(table.features.len());
                        let features: Vec<(FeatureKey, &str)> = table.features[start..end]
                            .iter()
                            .filter_map(|feature| {
                                feature.value.get(pool).map(|value| (feature.key, value))
                            })
                            .collect();
                        temp_tokens.push((
                            TokenAttrs {
                                node: token.node,
                                surface: token.surface,
                                lang: token.lang,
                                tagset: token.tagset,
                                flags: token.flags,
                            },
                            token.reading.get(pool),
                            token.base_form.get(pool),
                            features,
                        ));
                    }
                }

                // Sort by surface.start to preserve documented (node, surface.start) order.
                temp_tokens.sort_by_key(|(attrs, _, _, _)| attrs.surface.start);

                for (attrs, reading, base_form, features) in temp_tokens {
                    builder.push_token(attrs, reading, base_form, &features);
                }
            }
        }
        Ok(Some(builder.finish()))
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

    fn as_refs(boxes: &[Box<dyn Rule>]) -> Vec<&dyn Rule> {
        boxes.iter().map(|b| b.as_ref()).collect()
    }

    #[test]
    fn active_providers_is_the_registered_required_intersection() {
        let mut reg = MorphologyRegistry::new();
        reg.insert(provider(Lang::JA), DictId::from_pin([1; 32]));
        reg.insert(provider(Lang::KO), DictId::from_pin([2; 32]));

        // A JA-needing rule → exactly the JA provider.
        let ja = vec![morph_rule("ja", Lang::JA)];
        let active = reg.active_providers(&as_refs(&ja));
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].0, Lang::JA);
        assert_eq!(active[0].1.lang(), Lang::JA);

        // JA + KO needed → both, Lang-ascending (JA=0 before KO=1).
        let jako = vec![morph_rule("ja", Lang::JA), morph_rule("ko", Lang::KO)];
        let active = reg.active_providers(&as_refs(&jako));
        assert_eq!(
            active.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            vec![Lang::JA, Lang::KO]
        );

        // A required lang with no registered provider is dropped (only JA survives).
        let mut ja_only = MorphologyRegistry::new();
        ja_only.insert(provider(Lang::JA), DictId::from_pin([1; 32]));
        let active = ja_only.active_providers(&as_refs(&jako));
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].0, Lang::JA);

        // No rules → empty; empty registry → empty even when a rule needs JA.
        assert!(reg.active_providers(&[]).is_empty());
        assert!(
            MorphologyRegistry::new()
                .active_providers(&as_refs(&ja))
                .is_empty()
        );
    }

    #[test]
    fn build_table_sorts_tokens_by_surface_start_when_multiple_providers_active() {
        use tzlint_ast::morphology::{MorphologyBuilder, Tagset, TokenAttrs};
        use tzlint_ast::{NodeId, Span};
        use tzlint_pdk::MorphologyError;

        // A mock provider that returns a token starting at `start`
        struct StartMockProvider {
            lang: Lang,
            start: u32,
        }
        impl MorphologyProvider for StartMockProvider {
            fn lang(&self) -> Lang {
                self.lang
            }
            fn analyze(
                &self,
                _text: &str,
                base: u32,
                node: NodeId,
            ) -> Result<MorphologyV1, MorphologyError> {
                let mut builder = MorphologyBuilder::new();
                builder.push_token(
                    TokenAttrs {
                        node,
                        surface: Span::new(base + self.start, base + self.start + 1),
                        lang: self.lang,
                        tagset: Tagset::IPADIC,
                        flags: 0,
                    },
                    None,
                    None,
                    &[],
                );
                Ok(builder.finish())
            }
        }

        let mut reg = MorphologyRegistry::new();
        // JA provider returns a token starting at base + 5
        reg.insert(
            Box::new(StartMockProvider {
                lang: Lang::JA,
                start: 5,
            }),
            DictId::from_pin([1; 32]),
        );
        // KO provider returns a token starting at base + 2
        reg.insert(
            Box::new(StartMockProvider {
                lang: Lang::KO,
                start: 2,
            }),
            DictId::from_pin([2; 32]),
        );

        let rule_boxes = [morph_rule("ja", Lang::JA), morph_rule("ko", Lang::KO)];
        let rules = as_refs(&rule_boxes);

        let source = "0123456789";
        use tzlint_ast::{Node, OptionNodeId};
        let ast = tzlint_ast::Ast {
            nodes: vec![
                Node {
                    kind: NodeKind::ROOT,
                    span: Span::new(0, 10),
                    parent: NodeId(0),
                    first_child: OptionNodeId::some(NodeId(1)),
                    next_sibling: OptionNodeId::NONE,
                },
                Node {
                    kind: NodeKind::TEXT,
                    span: Span::new(0, 10),
                    parent: NodeId(0),
                    first_child: OptionNodeId::NONE,
                    next_sibling: OptionNodeId::NONE,
                },
            ],
            text: source.to_string(),
            root: NodeId(0),
        };
        let bytes = tzlint_ast::to_archive(&ast).unwrap();
        let archived = tzlint_ast::access(&bytes).unwrap();

        let table = reg.build_table(archived, &rules).unwrap().unwrap();
        assert_eq!(table.tokens.len(), 2);
        // KO token (start: 2) must come before JA token (start: 5)
        assert_eq!(table.tokens[0].lang, Lang::KO);
        assert_eq!(table.tokens[0].surface.start, 2);
        assert_eq!(table.tokens[1].lang, Lang::JA);
        assert_eq!(table.tokens[1].surface.start, 5);
    }
}
