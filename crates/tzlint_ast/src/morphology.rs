//! `MorphologyV1` — the frozen ABI for per-node morphological tokens (M2).
//!
//! This is an **additive table** in the sense of `abi-spec.md`: a *separate* archived payload
//! that sits alongside the [`Ast`](crate::Ast) core and is keyed by [`NodeId`], never a new
//! field on [`Node`](crate::Node) (extending the 24-byte node would break its golden layout).
//! A [`MorphologyProvider`](../../tzlint_pdk) tokenizes the text of a node and produces a
//! [`MorphologyV1`]; rules read the archived form ([`ArchivedMorphologyV1`]) in place.
//!
//! # Frozen layout
//!
//! The layout follows the same five locks as `AstCoreV1`:
//! 1. `#[derive(Archive, Serialize)]` only — **no `Deserialize`** (read in place, zero-copy).
//! 2. `no_std` + `alloc` (the crate posture), so it rides the plugin ABI to the wasm guest.
//! 3. Niche-free wire integers: [`Lang`]/[`Tagset`]/[`FeatureKey`] and `flags` are bare `u32`;
//!    the "optional" string fields are a [`StrRef`] with a `{0,0}` sentinel ([`StrRef::NONE`]),
//!    never `Option<String>`.
//! 4. Open enums ([`Lang`], [`Tagset`], [`FeatureKey`]) so a new language, dictionary, or feature
//!    key never forces a `V1 → V2` bump — unknown values are preserved (the "open features" intent).
//! 5. A `golden_archived_layout_is_frozen` test + `const` size/align asserts fail the build on
//!    any drift (including an rkyv upgrade).
//!
//! `surface` is a [`Span`] into `Ast::text` (index-into-text, like a node), not an owned string.
//! `reading`/`base_form`/feature values are [`StrRef`]s into the table's own [`strings`] pool —
//! they come from the dictionary, not the source, so they are stored once in the table.
//!
//! [`strings`]: MorphologyV1::strings

use alloc::string::String;
use alloc::vec::Vec;

use rkyv::{Archive, Serialize};

use crate::{NodeId, Span};

/// The `MorphologyV1` interface version (the `requires_interfaces` lock from `abi-spec.md`).
/// A future *additive* change that consumers must opt into bumps this; it is also folded into
/// the document cache key so an interface change invalidates stale entries.
pub const MORPHOLOGY_INTERFACE_VERSION: u32 = 1;

/// An open-enum language tag, a bare `u32` like [`NodeKind`](crate::NodeKind).
///
/// Forward-compatible: any value round-trips, and an unknown language from a newer producer is
/// preserved rather than rejected. The known values are the languages the morphology milestone
/// targets (Japanese first); a backend may cover several.
#[derive(Archive, Serialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Lang(u32);

impl Lang {
    /// Japanese.
    pub const JA: Self = Self(0);
    /// Korean.
    pub const KO: Self = Self(1);
    /// Chinese.
    pub const ZH: Self = Self(2);

    /// Number of known languages. Values `< COUNT` are known; `>= COUNT` is opaque.
    pub const COUNT: u32 = 3;

    /// The raw wire value.
    #[inline]
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
    /// Wrap a raw wire value (total: unknown values are preserved).
    #[inline]
    #[must_use]
    pub const fn from_u32(value: u32) -> Self {
        Self(value)
    }
    /// Whether this is one of the known [`Lang`] constants.
    #[inline]
    #[must_use]
    pub const fn is_known(self) -> bool {
        self.0 < Self::COUNT
    }
}

/// An open-enum dictionary/tagset tag, a bare `u32` like [`NodeKind`](crate::NodeKind).
///
/// A token's [`FeatureKey`] values, [`reading`](Token::reading) and [`base_form`](Token::base_form)
/// are defined by the *dictionary* that produced them, and dictionaries use different feature
/// schemas ("tagsets") — e.g. IPADIC's 9 positional columns vs UniDic's ~21 with a 4-level part of
/// speech and a separate lexeme/orthographic base form. Recording the tagset per token lets a rule
/// disambiguate cross-dictionary semantics; backends should still map their columns onto the
/// canonical [`FeatureKey`] meanings where they line up. Forward-compatible: unknown tagsets from a
/// newer producer are preserved, not rejected.
#[derive(Archive, Serialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tagset(u32);

impl Tagset {
    /// No specific dictionary (e.g. a dictionary-free provider such as the whitespace tokenizer).
    pub const NONE: Self = Self(0);
    /// IPADIC (mecab-ipadic): the classic 9-column Japanese tagset.
    pub const IPADIC: Self = Self(1);
    /// UniDic: a richer Japanese tagset (multi-level POS, lexeme vs orthographic base form).
    pub const UNIDIC: Self = Self(2);
    /// ko-dic: the mecab-ko Korean tagset.
    pub const KO_DIC: Self = Self(3);
    /// CC-CEDICT: a Chinese dictionary tagset.
    pub const CC_CEDICT: Self = Self(4);

    /// Number of known tagsets. Values `< COUNT` are known; `>= COUNT` is opaque.
    pub const COUNT: u32 = 5;

    /// The raw wire value.
    #[inline]
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
    /// Wrap a raw wire value (total: unknown values are preserved).
    #[inline]
    #[must_use]
    pub const fn from_u32(value: u32) -> Self {
        Self(value)
    }
    /// Whether this is one of the known [`Tagset`] constants.
    #[inline]
    #[must_use]
    pub const fn is_known(self) -> bool {
        self.0 < Self::COUNT
    }
}

/// An open-enum feature key, a bare `u32` like [`NodeKind`](crate::NodeKind).
///
/// Morphological features are stored as open `(key, value)` pairs (à la lindera's
/// `token.details()`), sidestepping a closed, per-tagset enum so adding a feature never forces a
/// `V1 → V2` bump. The known keys cover the common positional fields of dictionaries like IPADIC;
/// `reading` and `base_form` are promoted to dedicated [`Token`] fields and are **not** repeated
/// here. A backend may emit any additional key as an opaque value `>= COUNT`.
#[derive(Archive, Serialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FeatureKey(u32);

impl FeatureKey {
    /// Part of speech (e.g. IPADIC 品詞).
    pub const POS: Self = Self(0);
    /// Part-of-speech sub-classification 1 (品詞細分類1).
    pub const POS_SUB_1: Self = Self(1);
    /// Part-of-speech sub-classification 2 (品詞細分類2).
    pub const POS_SUB_2: Self = Self(2);
    /// Part-of-speech sub-classification 3 (品詞細分類3).
    pub const POS_SUB_3: Self = Self(3);
    /// Conjugation type (活用型).
    pub const CONJUGATION_TYPE: Self = Self(4);
    /// Conjugation form (活用形).
    pub const CONJUGATION_FORM: Self = Self(5);
    /// Pronunciation (発音).
    pub const PRONUNCIATION: Self = Self(6);

    /// Number of known keys. Values `< COUNT` are known; `>= COUNT` is opaque.
    pub const COUNT: u32 = 7;

    /// The raw wire value.
    #[inline]
    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
    /// Wrap a raw wire value (total: unknown values are preserved).
    #[inline]
    #[must_use]
    pub const fn from_u32(value: u32) -> Self {
        Self(value)
    }
    /// Whether this is one of the known [`FeatureKey`] constants.
    #[inline]
    #[must_use]
    pub const fn is_known(self) -> bool {
        self.0 < Self::COUNT
    }
}

/// A reference into the table's [`strings`](MorphologyV1::strings) pool: a `(offset, len)` byte
/// range, niche-free (a bare pair of `u32`).
///
/// The absent optional (a missing `reading`/`base_form`) is the `{0, 0}` sentinel
/// [`StrRef::NONE`] — **not** `Option<String>`, which rkyv would archive with a discriminant and
/// risk golden-byte instability (the same discipline as [`OptionNodeId`](crate::OptionNodeId)).
/// A real interned string always has `len > 0`, so `len == 0` unambiguously means "absent".
#[derive(Archive, Serialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StrRef {
    /// Byte offset into the strings pool.
    pub offset: u32,
    /// Byte length; `0` means absent (the [`NONE`](StrRef::NONE) sentinel).
    pub len: u32,
}

impl StrRef {
    /// The absent sentinel.
    pub const NONE: Self = Self { offset: 0, len: 0 };

    /// Whether this is the absent sentinel.
    #[inline]
    #[must_use]
    pub const fn is_none(self) -> bool {
        self.len == 0
    }

    /// Resolve against `pool`, or `None` if absent or out of range (never panics).
    #[inline]
    #[must_use]
    pub fn get(self, pool: &str) -> Option<&str> {
        if self.is_none() {
            return None;
        }
        let start = self.offset as usize;
        let end = start.saturating_add(self.len as usize);
        pool.get(start..end)
    }
}

/// One open `(key, value)` morphological feature; `value` is a [`StrRef`] into the strings pool.
#[derive(Archive, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
    /// The feature key (open enum).
    pub key: FeatureKey,
    /// The feature value, interned in the table's strings pool.
    pub value: StrRef,
}

/// One morphological token.
///
/// `surface` is an absolute [`Span`] into `Ast::text` (index-into-text, like a node), so the
/// archive stays compact. `reading`/`base_form` are [`StrRef`]s (`NONE` when absent). The token's
/// features are the contiguous slice `[features_start, features_start + features_len)` of
/// [`MorphologyV1::features`] (an offset-table, not a per-token `Vec`). `node` is the owning AST
/// node (the table is keyed by [`NodeId`]); `tagset` records which dictionary defined the
/// features; `flags` is a bitfield ([`FLAG_UNKNOWN`](Token::FLAG_UNKNOWN)).
#[derive(Archive, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The owning AST node.
    pub node: NodeId,
    /// Absolute byte range of the surface form in `Ast::text`.
    pub surface: Span,
    /// The language this token was analyzed as.
    pub lang: Lang,
    /// The dictionary/tagset that defined this token's features.
    pub tagset: Tagset,
    /// Bitfield of per-token flags (see [`FLAG_UNKNOWN`](Token::FLAG_UNKNOWN)); unused bits are 0
    /// and reserved.
    pub flags: u32,
    /// Reading (e.g. katakana), or [`StrRef::NONE`].
    pub reading: StrRef,
    /// Dictionary base form / lemma, or [`StrRef::NONE`].
    pub base_form: StrRef,
    /// Index of the first feature in [`MorphologyV1::features`].
    pub features_start: u32,
    /// Number of features for this token.
    pub features_len: u32,
}

impl Token {
    /// `flags` bit: the token is **unknown** to the dictionary — an out-of-vocabulary run the
    /// analyzer guessed rather than looked up (lindera's `is_unknown`). Rules that should not
    /// trust the features of guessed tokens test this bit.
    pub const FLAG_UNKNOWN: u32 = 1 << 0;
}

/// The scalar attributes of a token, passed to [`MorphologyBuilder::push_token`]. (Its string
/// fields — `reading`/`base_form`/feature values — are interned separately so the builder owns
/// the pool.) Bundled into one struct to keep `push_token` under the argument-count lint.
#[derive(Debug, Clone, Copy)]
pub struct TokenAttrs {
    /// The owning AST node.
    pub node: NodeId,
    /// Absolute byte range of the surface form in `Ast::text`.
    pub surface: Span,
    /// The language this token was analyzed as.
    pub lang: Lang,
    /// The dictionary/tagset that defined this token's features.
    pub tagset: Tagset,
    /// Bitfield of per-token flags (see [`Token::FLAG_UNKNOWN`]).
    pub flags: u32,
}

/// The frozen `MorphologyV1` table: a separate archived payload alongside `AstCoreV1`.
///
/// Tokens are emitted in `(node, surface.start)` order, so the tokens of one node form a
/// contiguous run. `features` is the flat backing store for [`Token::features_start`]/
/// [`Token::features_len`]; `strings` is the shared pool that every [`StrRef`] indexes.
#[derive(Archive, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct MorphologyV1 {
    /// The interface version ([`MORPHOLOGY_INTERFACE_VERSION`]).
    pub interface_version: u32,
    /// All tokens, in `(node, surface.start)` order.
    pub tokens: Vec<Token>,
    /// Flat feature backing store indexed by [`Token::features_start`]/[`Token::features_len`].
    pub features: Vec<Feature>,
    /// The string pool that [`StrRef`] and [`Feature::value`] index into.
    pub strings: String,
}

/// Builds a [`MorphologyV1`], interning strings into the pool and tracking feature ranges.
///
/// This is the one construction path (used by the test provider and, later, real backends), so
/// the offset/range bookkeeping lives in one place. The strings pool is addressed by `u32`
/// offsets/lengths ([`StrRef`] is a frozen pair of `u32`); the whole AST text is itself
/// `u32`-bounded ([`Span`]), so a realistic pool always fits. A pathological pool exceeding
/// `u32::MAX` bytes would truncate an offset — a `debug_assert` in `intern` catches that in tests.
#[derive(Debug, Default, Clone)]
pub struct MorphologyBuilder {
    tokens: Vec<Token>,
    features: Vec<Feature>,
    strings: String,
}

impl MorphologyBuilder {
    /// A new, empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern `s` into the pool and return a [`StrRef`]; an empty string is [`StrRef::NONE`].
    fn intern(&mut self, s: &str) -> StrRef {
        if s.is_empty() {
            return StrRef::NONE;
        }
        // The pool is u32-addressed (a frozen StrRef is a pair of u32). This only fires for a
        // pathological (>4 GiB) pool; the assert catches it in tests rather than silently
        // truncating into a corrupt-but-valid-looking archive.
        debug_assert!(
            self.strings
                .len()
                .checked_add(s.len())
                .is_some_and(|total| total <= u32::MAX as usize),
            "morphology string pool exceeded u32 addressing"
        );
        let offset = self.strings.len() as u32;
        self.strings.push_str(s);
        StrRef {
            offset,
            len: s.len() as u32,
        }
    }

    /// Append one token. `reading`/`base_form` of `None` (or `Some("")`) become [`StrRef::NONE`].
    /// `features` are appended to the flat store and referenced by the token's range.
    pub fn push_token(
        &mut self,
        attrs: TokenAttrs,
        reading: Option<&str>,
        base_form: Option<&str>,
        features: &[(FeatureKey, &str)],
    ) {
        let reading = reading.map_or(StrRef::NONE, |s| self.intern(s));
        let base_form = base_form.map_or(StrRef::NONE, |s| self.intern(s));
        let features_start = self.features.len() as u32;
        for (key, value) in features {
            let value = self.intern(value);
            self.features.push(Feature { key: *key, value });
        }
        let features_len = features.len() as u32;
        self.tokens.push(Token {
            node: attrs.node,
            surface: attrs.surface,
            lang: attrs.lang,
            tagset: attrs.tagset,
            flags: attrs.flags,
            reading,
            base_form,
            features_start,
            features_len,
        });
    }

    /// Finish into a [`MorphologyV1`].
    #[must_use]
    pub fn finish(self) -> MorphologyV1 {
        MorphologyV1 {
            interface_version: MORPHOLOGY_INTERFACE_VERSION,
            tokens: self.tokens,
            features: self.features,
            strings: self.strings,
        }
    }
}

// ── Zero-copy accessors over the archived form ───────────────────────────────────────

impl ArchivedLang {
    /// Decode to a native [`Lang`].
    #[inline]
    #[must_use]
    pub const fn get(&self) -> Lang {
        Lang(self.0.to_native())
    }
}

impl ArchivedTagset {
    /// Decode to a native [`Tagset`].
    #[inline]
    #[must_use]
    pub const fn get(&self) -> Tagset {
        Tagset(self.0.to_native())
    }
}

impl ArchivedFeatureKey {
    /// Decode to a native [`FeatureKey`].
    #[inline]
    #[must_use]
    pub const fn get(&self) -> FeatureKey {
        FeatureKey(self.0.to_native())
    }
}

impl ArchivedStrRef {
    /// Whether this is the absent sentinel.
    #[inline]
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.len.to_native() == 0
    }

    /// Resolve against `pool`, or `None` if absent or out of range (never panics).
    #[inline]
    #[must_use]
    pub fn get<'a>(&self, pool: &'a str) -> Option<&'a str> {
        let len = self.len.to_native() as usize;
        if len == 0 {
            return None;
        }
        let start = self.offset.to_native() as usize;
        pool.get(start..start.saturating_add(len))
    }
}

impl ArchivedFeature {
    /// The feature key.
    #[inline]
    #[must_use]
    pub fn key(&self) -> FeatureKey {
        self.key.get()
    }
    /// The feature value resolved against `pool`, or `None`.
    #[inline]
    #[must_use]
    pub fn value<'a>(&self, pool: &'a str) -> Option<&'a str> {
        self.value.get(pool)
    }
}

impl ArchivedToken {
    /// The owning AST node.
    #[inline]
    #[must_use]
    pub fn node(&self) -> NodeId {
        self.node.get()
    }
    /// The surface [`Span`] into `Ast::text`.
    #[inline]
    #[must_use]
    pub fn surface(&self) -> Span {
        self.surface.get()
    }
    /// The language this token was analyzed as.
    #[inline]
    #[must_use]
    pub fn lang(&self) -> Lang {
        self.lang.get()
    }
    /// The dictionary/tagset that defined this token's features.
    #[inline]
    #[must_use]
    pub fn tagset(&self) -> Tagset {
        self.tagset.get()
    }
    /// The raw per-token flags bitfield.
    #[inline]
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.flags.to_native()
    }
    /// Whether the token is unknown to the dictionary (OOV / guessed) — see
    /// [`Token::FLAG_UNKNOWN`].
    #[inline]
    #[must_use]
    pub fn is_unknown(&self) -> bool {
        self.flags() & Token::FLAG_UNKNOWN != 0
    }
    /// The reading, resolved against `table`'s pool, or `None`.
    #[inline]
    #[must_use]
    pub fn reading<'a>(&self, table: &'a ArchivedMorphologyV1) -> Option<&'a str> {
        self.reading.get(table.strings())
    }
    /// The base form / lemma, resolved against `table`'s pool, or `None`.
    #[inline]
    #[must_use]
    pub fn base_form<'a>(&self, table: &'a ArchivedMorphologyV1) -> Option<&'a str> {
        self.base_form.get(table.strings())
    }
    /// The token's `(key, value)` features (value resolved against `table`'s pool). Out-of-range
    /// indices are clamped, so a malformed table yields fewer features rather than a panic.
    pub fn features<'a>(
        &self,
        table: &'a ArchivedMorphologyV1,
    ) -> impl Iterator<Item = (FeatureKey, Option<&'a str>)> + 'a {
        let all = table.features();
        let start = (self.features_start.to_native() as usize).min(all.len());
        let end = start
            .saturating_add(self.features_len.to_native() as usize)
            .min(all.len());
        let pool = table.strings();
        all[start..end]
            .iter()
            .map(move |f| (f.key(), f.value(pool)))
    }
}

impl ArchivedMorphologyV1 {
    /// The interface version.
    #[inline]
    #[must_use]
    pub fn interface_version(&self) -> u32 {
        self.interface_version.to_native()
    }
    /// The string pool.
    #[inline]
    #[must_use]
    pub fn strings(&self) -> &str {
        self.strings.as_ref()
    }
    /// All tokens, in `(node, surface.start)` order.
    #[inline]
    #[must_use]
    pub fn tokens(&self) -> &[ArchivedToken] {
        self.tokens.as_slice()
    }
    /// The flat feature store.
    #[inline]
    #[must_use]
    pub fn features(&self) -> &[ArchivedFeature] {
        self.features.as_slice()
    }
    /// Whether the table holds no tokens.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
    /// The tokens that belong to `node`.
    pub fn tokens_of(&self, node: NodeId) -> impl Iterator<Item = &ArchivedToken> {
        self.tokens().iter().filter(move |t| t.node() == node)
    }
}

// ── Archive bridge ───────────────────────────────────────────────────────────────────
// Parallel to the `Ast` bridge, so the pinned rkyv configuration stays centralized.

use crate::AlignedVec;

/// Serialize a [`MorphologyV1`] into its frozen archive (aligned for [`access_morphology`]).
pub fn to_archive_morphology(table: &MorphologyV1) -> Result<AlignedVec, rkyv::rancor::Error> {
    rkyv::to_bytes::<rkyv::rancor::Error>(table)
}

/// Access an archived [`MorphologyV1`] in place (zero-copy), validating the bytes (`bytecheck`)
/// so a malformed or misaligned buffer is a recoverable `Err`, never UB.
pub fn access_morphology(bytes: &[u8]) -> Result<&ArchivedMorphologyV1, rkyv::rancor::Error> {
    rkyv::access::<ArchivedMorphologyV1, rkyv::rancor::Error>(bytes)
}

// ── Frozen-layout guards ─────────────────────────────────────────────────────────────
// Compile-time assertions on the archived record layout — these fail the BUILD on any drift.
const _: () = {
    use core::mem::{align_of, size_of};
    assert!(size_of::<ArchivedLang>() == 4);
    assert!(size_of::<ArchivedTagset>() == 4);
    assert!(size_of::<ArchivedFeatureKey>() == 4);
    assert!(size_of::<ArchivedStrRef>() == 8);
    // key(4) + value{offset,len}(8) = 12.
    assert!(size_of::<ArchivedFeature>() == 12);
    assert!(align_of::<ArchivedFeature>() == 4);
    // node(4) + surface(8) + lang(4) + tagset(4) + flags(4) + reading(8) + base_form(8)
    // + start(4) + len(4) = 48.
    assert!(size_of::<ArchivedToken>() == 48);
    assert!(align_of::<ArchivedToken>() == 4);
};

#[cfg(test)]
mod tests {
    use super::*;

    /// A canonical 2-token table: token0 an IPADIC token with a reading and one POS feature;
    /// token1 a bare unknown (OOV) token. Short ASCII pool values keep the golden image legible,
    /// and the pair exercises `tagset`, `flags`, reading/feature presence and absence.
    fn canonical_table() -> MorphologyV1 {
        let mut b = MorphologyBuilder::new();
        b.push_token(
            TokenAttrs {
                node: NodeId(0),
                surface: Span::new(0, 2),
                lang: Lang::JA,
                tagset: Tagset::IPADIC,
                flags: 0,
            },
            Some("yo"),
            None,
            &[(FeatureKey::POS, "n")],
        );
        b.push_token(
            TokenAttrs {
                node: NodeId(0),
                surface: Span::new(3, 5),
                lang: Lang::JA,
                tagset: Tagset::IPADIC,
                flags: Token::FLAG_UNKNOWN,
            },
            None,
            None,
            &[],
        );
        b.finish()
    }

    #[test]
    fn open_enums_roundtrip_and_tolerate_unknown() {
        for l in [Lang::JA, Lang::KO, Lang::ZH] {
            assert_eq!(Lang::from_u32(l.as_u32()), l);
            assert!(l.is_known());
        }
        assert_eq!(Lang::COUNT, 3);
        assert!(!Lang::from_u32(99).is_known());
        assert_eq!(Lang::from_u32(99).as_u32(), 99);

        for k in [FeatureKey::POS, FeatureKey::PRONUNCIATION] {
            assert_eq!(FeatureKey::from_u32(k.as_u32()), k);
            assert!(k.is_known());
        }
        assert_eq!(FeatureKey::COUNT, 7);
        assert!(!FeatureKey::from_u32(7).is_known());
        assert_eq!(FeatureKey::from_u32(1234).as_u32(), 1234);

        for t in [
            Tagset::NONE,
            Tagset::IPADIC,
            Tagset::UNIDIC,
            Tagset::KO_DIC,
            Tagset::CC_CEDICT,
        ] {
            assert_eq!(Tagset::from_u32(t.as_u32()), t);
            assert!(t.is_known());
        }
        assert_eq!(Tagset::COUNT, 5);
        assert!(!Tagset::from_u32(5).is_known());
        assert_eq!(Tagset::from_u32(77).as_u32(), 77);
    }

    #[test]
    fn strref_sentinel_and_resolution() {
        assert!(StrRef::NONE.is_none());
        assert_eq!(StrRef::NONE.get("anything"), None);
        let r = StrRef { offset: 2, len: 3 };
        assert!(!r.is_none());
        assert_eq!(r.get("ab cde fg"), Some(" cd"));
        // Out-of-range resolves to None, never panics.
        assert_eq!(StrRef { offset: 8, len: 5 }.get("short"), None);
    }

    #[test]
    fn builder_interns_and_ranges_features() {
        let table = canonical_table();
        assert_eq!(table.interface_version, MORPHOLOGY_INTERFACE_VERSION);
        assert_eq!(table.tokens.len(), 2);
        assert_eq!(table.features.len(), 1);
        // "yo" then "n" → pool "yon".
        assert_eq!(table.strings, "yon");
        assert_eq!(table.tokens[0].reading.get(&table.strings), Some("yo"));
        assert!(table.tokens[0].base_form.is_none());
        assert_eq!(table.tokens[0].features_start, 0);
        assert_eq!(table.tokens[0].features_len, 1);
        assert_eq!(table.tokens[1].features_len, 0);
        assert_eq!(table.features[0].key, FeatureKey::POS);
        assert_eq!(table.features[0].value.get(&table.strings), Some("n"));
        // tagset / flags are recorded per token.
        assert_eq!(table.tokens[0].tagset, Tagset::IPADIC);
        assert_eq!(table.tokens[0].flags, 0);
        assert_eq!(table.tokens[1].flags, Token::FLAG_UNKNOWN);
    }

    #[test]
    fn builder_collapses_empty_strings_to_absent() {
        // A `Some("")` reading/base form interns to the absent sentinel (the builder collapses an
        // empty value rather than storing a 0-length ref; providers should omit empties anyway).
        let mut b = MorphologyBuilder::new();
        b.push_token(
            TokenAttrs {
                node: NodeId(0),
                surface: Span::new(0, 1),
                lang: Lang::JA,
                tagset: Tagset::NONE,
                flags: 0,
            },
            Some(""),
            Some(""),
            &[],
        );
        let table = b.finish();
        assert!(table.tokens[0].reading.is_none());
        assert!(table.tokens[0].base_form.is_none());
        assert!(table.strings.is_empty());
    }

    #[test]
    fn checked_access_roundtrip_and_accessors() {
        let table = canonical_table();
        let bytes = to_archive_morphology(&table).unwrap();
        let archived = access_morphology(&bytes).unwrap();

        assert_eq!(archived.interface_version(), MORPHOLOGY_INTERFACE_VERSION);
        assert_eq!(archived.strings(), "yon");
        assert_eq!(archived.tokens().len(), 2);
        assert!(!archived.is_empty());

        let t0 = &archived.tokens()[0];
        assert_eq!(t0.node(), NodeId(0));
        assert_eq!(t0.surface(), Span::new(0, 2));
        assert_eq!(t0.lang(), Lang::JA);
        assert_eq!(t0.tagset(), Tagset::IPADIC);
        assert_eq!(t0.flags(), 0);
        assert!(!t0.is_unknown());
        assert_eq!(t0.reading(archived), Some("yo"));
        assert_eq!(t0.base_form(archived), None);
        let feats: Vec<_> = t0.features(archived).collect();
        assert_eq!(feats, vec![(FeatureKey::POS, Some("n"))]);

        let t1 = &archived.tokens()[1];
        assert_eq!(t1.surface(), Span::new(3, 5));
        assert!(t1.is_unknown());
        assert!(t1.reading.is_none()); // archived-side StrRef sentinel check
        assert_eq!(t1.reading(archived), None);
        assert_eq!(t1.features(archived).count(), 0);

        // Both tokens belong to node 0.
        assert_eq!(archived.tokens_of(NodeId(0)).count(), 2);
        assert_eq!(archived.tokens_of(NodeId(1)).count(), 0);
    }

    #[test]
    fn checked_access_rejects_garbage() {
        let garbage = [0xFFu8; 12];
        assert!(access_morphology(&garbage).is_err());
    }

    #[test]
    fn golden_archived_layout_is_frozen() {
        let bytes = to_archive_morphology(&canonical_table()).unwrap();
        // Regenerate ONLY on a deliberate ABI change: run, copy the printed `actual`, and bump
        // MORPHOLOGY_INTERFACE_VERSION. Accidental drift (field reorder, rkyv upgrade) trips this.
        //
        // Layout (little-endian; 48-byte tokens, 12-byte features, {0,0} = absent StrRef):
        #[rustfmt::skip]
        const EXPECTED: &[u8] = &[
            // token 0: node=0 surface{0,2} lang=0 tagset=IPADIC(1) flags=0 reading{off0,len2} base=NONE fstart=0 flen=1
            0,0,0,0,  0,0,0,0,2,0,0,0,  0,0,0,0,  1,0,0,0,  0,0,0,0,  0,0,0,0,2,0,0,0,  0,0,0,0,0,0,0,0,  0,0,0,0, 1,0,0,0,
            // token 1: node=0 surface{3,5} lang=0 tagset=IPADIC(1) flags=UNKNOWN(1) reading=NONE base=NONE fstart=1 flen=0
            0,0,0,0,  3,0,0,0,5,0,0,0,  0,0,0,0,  1,0,0,0,  1,0,0,0,  0,0,0,0,0,0,0,0,  0,0,0,0,0,0,0,0,  1,0,0,0, 0,0,0,0,
            // feature 0: key=POS(0)  value{off2,len1}
            0,0,0,0,  2,0,0,0,1,0,0,0,
            // root MorphologyV1.interface_version = 1
            1,0,0,0,
            // tokens: ArchivedVec { rel_ptr = -112 (→ token 0), len = 2 }
            144,255,255,255, 2,0,0,0,
            // features: ArchivedVec { rel_ptr = -24 (→ feature 0), len = 1 }
            232,255,255,255, 1,0,0,0,
            // strings: ArchivedString — "yon" stored inline (rkyv small-string optimization)
            121,111,110,255, 255,255,255,255,
        ];

        assert_eq!(
            bytes.as_slice(),
            EXPECTED,
            "MorphologyV1 wire layout changed — actual ({} bytes) = {:?}",
            bytes.len(),
            bytes.as_slice(),
        );
    }
}
