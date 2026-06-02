//! The morphology provider seam (M2): the trait a tokenizer backend implements, plus a
//! dictionary-free test provider.
//!
//! A [`MorphologyProvider`] turns the text of a node into a frozen
//! [`MorphologyV1`](tzlint_ast::morphology::MorphologyV1) table that rules read in place. The
//! trait is `no_std`/alloc and backend-agnostic; heavy native backends (lindera, …) and
//! dictionary provisioning live in `tzlint_core` behind this seam and are injected Host-style,
//! so the core stays wasm-clean and dictionary-free by default. The deterministic
//! [`WhitespaceProvider`] lets the model and the (later) engine wiring be tested without any
//! dictionary.

use alloc::format;
use alloc::string::String;

use tzlint_ast::morphology::{Lang, MorphologyBuilder, MorphologyV1, Tagset, TokenAttrs};
use tzlint_ast::{NodeId, Span};

/// Why a [`MorphologyProvider`] could not analyze text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MorphologyError {
    /// The backend failed (its error rendered into a message).
    Backend(String),
    /// No dictionary is available for the requested language.
    DictionaryUnavailable(Lang),
}

impl core::fmt::Display for MorphologyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MorphologyError::Backend(msg) => write!(f, "morphology backend error: {msg}"),
            MorphologyError::DictionaryUnavailable(lang) => {
                write!(f, "no dictionary for language {}", lang.as_u32())
            }
        }
    }
}

/// A morphological analyzer for one language.
///
/// `analyze` tokenizes `text` — whose first byte is at absolute `base_offset` in `Ast::text` —
/// and attaches every produced token to `node`, so each token's surface is an absolute
/// [`Span`] into the source and carries its owning [`NodeId`] (the table is keyed by node).
///
/// Contract for implementations:
/// - **Never panic.** A backend failure (including an input whose absolute offsets would exceed
///   the `u32` address space) is returned as `Err`, never a panic.
/// - **Valid spans.** Each token surface must satisfy `start <= end` and lie within the document;
///   never emit an inverted or overflowing span.
/// - **Omit empty/placeholder features.** A feature present in the produced table always has a
///   non-empty value — drop dictionary placeholders such as IPADIC's `*` rather than interning
///   them — so [`StrRef::NONE`](tzlint_ast::morphology::StrRef::NONE) (an absent reading/base
///   form or feature) is never confused with a present-but-empty value.
pub trait MorphologyProvider: Send + Sync {
    /// The language this provider analyzes.
    fn lang(&self) -> Lang;

    /// Tokenize `text` (at absolute `base_offset`, owned by `node`) into a [`MorphologyV1`].
    fn analyze(
        &self,
        text: &str,
        base_offset: u32,
        node: NodeId,
    ) -> Result<MorphologyV1, MorphologyError>;
}

/// A dictionary-free provider that tokenizes on whitespace: each maximal run of non-whitespace
/// characters becomes one token whose surface is its [`Span`], with no reading/base-form/features.
///
/// Deterministic and `no_std`, so it exercises the [`MorphologyV1`] model and the engine wiring
/// (later sub-steps) without any backend or dictionary.
#[derive(Debug, Clone, Copy)]
pub struct WhitespaceProvider {
    lang: Lang,
}

impl WhitespaceProvider {
    /// A provider tagging its tokens with `lang`.
    #[must_use]
    pub const fn new(lang: Lang) -> Self {
        Self { lang }
    }
}

impl MorphologyProvider for WhitespaceProvider {
    fn lang(&self) -> Lang {
        self.lang
    }

    fn analyze(
        &self,
        text: &str,
        base_offset: u32,
        node: NodeId,
    ) -> Result<MorphologyV1, MorphologyError> {
        // Guard the absolute-offset arithmetic up front: surfaces are `u32` Spans into
        // `Ast::text`, so a node whose text would push an offset past `u32::MAX` cannot be
        // represented. Surface that as an error rather than panicking (debug) or producing an
        // inverted span (release). Once `base_offset + text.len()` fits, every `base_offset + i`
        // below (with `i <= text.len()`) fits too, so the inner additions cannot overflow.
        let end_offset = u32::try_from(text.len())
            .ok()
            .and_then(|n| base_offset.checked_add(n))
            .ok_or_else(|| {
                MorphologyError::Backend(format!(
                    "node text at offset {base_offset} exceeds the u32 address space"
                ))
            })?;

        // A dictionary-free tokenizer: no tagset, and tokens are never dictionary-"unknown".
        let attrs = |surface| TokenAttrs {
            node,
            surface,
            lang: self.lang,
            tagset: Tagset::NONE,
            flags: 0,
        };

        let mut builder = MorphologyBuilder::new();
        let mut run_start: Option<usize> = None;
        for (i, c) in text.char_indices() {
            if c.is_whitespace() {
                if let Some(start) = run_start.take() {
                    builder.push_token(
                        attrs(Span::new(
                            base_offset + start as u32,
                            base_offset + i as u32,
                        )),
                        None,
                        None,
                        &[],
                    );
                }
            } else if run_start.is_none() {
                run_start = Some(i);
            }
        }
        if let Some(start) = run_start {
            builder.push_token(
                attrs(Span::new(base_offset + start as u32, end_offset)),
                None,
                None,
                &[],
            );
        }
        Ok(builder.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tzlint_ast::morphology::access_morphology;

    #[test]
    fn display_renders_errors() {
        assert_eq!(
            MorphologyError::Backend("boom".into()).to_string(),
            "morphology backend error: boom"
        );
        assert_eq!(
            MorphologyError::DictionaryUnavailable(Lang::JA).to_string(),
            "no dictionary for language 0"
        );
    }

    #[test]
    fn whitespace_provider_tokenizes_with_byte_spans() {
        let p = WhitespaceProvider::new(Lang::JA);
        assert_eq!(p.lang(), Lang::JA);
        let table = p.analyze("ab  cd", 0, NodeId(0)).unwrap();
        assert_eq!(table.tokens.len(), 2);
        assert_eq!(table.tokens[0].surface, Span::new(0, 2));
        assert_eq!(table.tokens[1].surface, Span::new(4, 6));
        assert!(table.tokens[0].reading.is_none());
        assert!(table.tokens[0].base_form.is_none());
        assert_eq!(table.tokens[0].features_len, 0);
        assert_eq!(table.tokens[0].lang, Lang::JA);
        // A dictionary-free provider tags no tagset and never marks tokens unknown.
        assert_eq!(table.tokens[0].tagset, Tagset::NONE);
        assert_eq!(table.tokens[0].flags, 0);
        // Every token is attached to the analyzed node.
        assert!(table.tokens.iter().all(|t| t.node == NodeId(0)));
    }

    #[test]
    fn whitespace_provider_honors_base_offset_and_multibyte() {
        // base_offset shifts surfaces into the document; CJK runs keep correct byte spans.
        let p = WhitespaceProvider::new(Lang::JA);
        let table = p.analyze("見出し テスト", 10, NodeId(3)).unwrap();
        assert_eq!(table.tokens.len(), 2);
        // "見出し" = 9 bytes at offset 10..19; space at 19; "テスト" = 9 bytes at 20..29.
        assert_eq!(table.tokens[0].surface, Span::new(10, 19));
        assert_eq!(table.tokens[1].surface, Span::new(20, 29));
        assert!(table.tokens.iter().all(|t| t.node == NodeId(3)));
    }

    #[test]
    fn whitespace_provider_errors_on_offset_overflow_instead_of_panicking() {
        // A node whose text would push a surface past u32::MAX is an Err, never a panic or an
        // inverted span (the no-panic / valid-span contract).
        let p = WhitespaceProvider::new(Lang::JA);
        let err = p.analyze("abcde", u32::MAX - 2, NodeId(0)).unwrap_err();
        assert!(matches!(err, MorphologyError::Backend(_)), "{err}");
    }

    #[test]
    fn whitespace_provider_empty_and_blank_inputs() {
        let p = WhitespaceProvider::new(Lang::JA);
        assert_eq!(p.analyze("", 0, NodeId(0)).unwrap().tokens.len(), 0);
        assert_eq!(p.analyze("   \n\t ", 0, NodeId(0)).unwrap().tokens.len(), 0);
    }

    #[test]
    fn whitespace_provider_output_archives_and_reads_back() {
        // The produced table is a valid frozen MorphologyV1: archive it and read via the
        // checked accessor, end-to-end with no dictionary.
        let p = WhitespaceProvider::new(Lang::JA);
        let table = p.analyze("one two", 0, NodeId(0)).unwrap();
        let bytes = tzlint_ast::morphology::to_archive_morphology(&table).unwrap();
        let archived = access_morphology(&bytes).unwrap();
        assert_eq!(archived.tokens_of(NodeId(0)).count(), 2);
        let first = &archived.tokens()[0];
        assert_eq!(first.surface(), Span::new(0, 3));
        assert_eq!(first.reading(archived), None);
        assert_eq!(first.features(archived).count(), 0);
    }
}
