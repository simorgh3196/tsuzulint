//! A tiny, self-describing container that bundles a morphology dictionary's component files into
//! one byte blob — the *content* of the decompressed `.dict.zst` that
//! [`provision_dictionary`](crate::provision_dictionary) returns.
//!
//! A native tokenizer backend (in `tzlint_morphology_native`) needs several separate
//! component files (a prefix-dictionary trie, a connection-cost matrix, character/unknown-word
//! tables, metadata). Rather than ship a directory or a tar — neither of which a browser/wasm
//! embedder can use — those components are concatenated here into a single blob with a fixed
//! header, so the whole dictionary travels as one hash-pinned, compressed artifact and is split
//! back apart **in memory** at load time. This module is therefore deliberately backend-agnostic:
//! it knows *byte ranges*, not any tokenizer; it names no tokenizer type and touches neither
//! [`Host`](crate::Host) nor the network, so it compiles for `wasm32` and a future browser backend
//! reuses the exact same split.
//!
//! # Format (all integers little-endian)
//!
//! ```text
//! offset  size  field
//! 0       8     magic = b"TZDICTC1"   (the trailing digit is the format version; a bump is a new magic)
//! 8       2     version: u16 = 1      (must equal 1)
//! 10      2     member_count: u16 = 8 (must equal 8 — the canonical component set)
//! 12      64    member table: 8 × { offset: u32, len: u32 }
//! 76      …     payload: the 8 member byte ranges
//! ```
//!
//! Members are **positional** (identified by table index, not by an embedded name string), which
//! removes all name-decoding surface. The canonical order matches the backend dictionary's component
//! load order; see [`Member`].
//!
//! # Untrusted bytes
//!
//! The pin authenticates the *compressed* artifact, but [`parse`] still treats its input as
//! untrusted and is **panic-free on any byte string**: every field is read through bounds-checked
//! slicing, every member range is validated with checked arithmetic against the blob length, and a
//! hostile length never drives an allocation (members are returned as borrowed slices, so the
//! parser allocates nothing). Member *content* validity (a malformed trie or archive) is the
//! backend loader's contract, not this codec's.

use core::fmt;

/// The 8-byte magic. The trailing `1` is the format version; a future incompatible layout uses a
/// new magic (and `version`), so the two can never be confused.
const MAGIC: &[u8; 8] = b"TZDICTC1";

/// The only supported [`MAGIC`]-companion version.
const VERSION: u16 = 1;

/// The fixed component set: exactly this many members, in this order.
pub const MEMBER_COUNT: usize = 8;

/// Bytes before the payload: magic (8) + version (2) + count (2) + table (`MEMBER_COUNT` × 8).
const HEADER_LEN: usize = 8 + 2 + 2 + MEMBER_COUNT * 8;

/// The canonical members, in table order. The discriminant is the table index, and the order
/// matches the component load order of the backend's IPADIC dictionary (`metadata` first, then the
/// prefix-dictionary quartet, the connection matrix, and the two character tables).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Member {
    /// `metadata.json` — dictionary metadata (JSON).
    Metadata = 0,
    /// `dict.da` — prefix-dictionary double-array trie.
    PrefixDa = 1,
    /// `dict.vals` — prefix-dictionary value blob.
    PrefixVals = 2,
    /// `dict.wordsidx` — prefix-dictionary word-index blob.
    PrefixWordsIdx = 3,
    /// `dict.words` — prefix-dictionary word-detail blob.
    PrefixWords = 4,
    /// `matrix.mtx` — connection-cost matrix.
    ConnectionMatrix = 5,
    /// `char_def.bin` — character-definition table.
    CharDef = 6,
    /// `unk.bin` — unknown-word dictionary.
    Unknown = 7,
}

/// A failure to parse or build a [`DictContainer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerError {
    /// The blob is shorter than a member needs (header or a member range runs past the end).
    Truncated {
        /// The byte length the read required.
        need: usize,
        /// The byte length actually available.
        got: usize,
    },
    /// The leading 8 bytes are not the container magic.
    BadMagic,
    /// The version field is not a supported value.
    UnsupportedVersion(u16),
    /// The member count is not the canonical [`MEMBER_COUNT`].
    BadMemberCount(u16),
    /// A member's `offset + len` overflows past the addressable range.
    MemberRangeOverflow {
        /// The offending member index.
        index: usize,
    },
    /// Building a container whose total size would exceed `u32` member offsets.
    EncodeOverflow,
}

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContainerError::Truncated { need, got } => {
                write!(
                    f,
                    "dictionary container truncated: need {need} bytes, got {got}"
                )
            }
            ContainerError::BadMagic => write!(f, "not a dictionary container (bad magic)"),
            ContainerError::UnsupportedVersion(v) => {
                write!(f, "unsupported dictionary container version {v}")
            }
            ContainerError::BadMemberCount(n) => {
                write!(
                    f,
                    "dictionary container must have {MEMBER_COUNT} members, found {n}"
                )
            }
            ContainerError::MemberRangeOverflow { index } => {
                write!(f, "dictionary container member {index} range overflows")
            }
            ContainerError::EncodeOverflow => {
                write!(
                    f,
                    "dictionary container too large to encode (member offset exceeds u32)"
                )
            }
        }
    }
}

impl std::error::Error for ContainerError {}

/// A parsed container: borrowed byte slices for each of the [`MEMBER_COUNT`] members.
///
/// Built by [`parse`]; the slices borrow the input blob (zero-copy). Access members by their
/// [`Member`] role via [`DictContainer::member`] or the named accessors.
#[derive(Debug, Clone, Copy)]
pub struct DictContainer<'a> {
    members: [&'a [u8]; MEMBER_COUNT],
}

impl<'a> DictContainer<'a> {
    /// The bytes of `member`.
    #[must_use]
    pub fn member(&self, member: Member) -> &'a [u8] {
        // `member as usize` is always in `0..MEMBER_COUNT` (the enum has exactly that many
        // variants), so this index never panics.
        self.members[member as usize]
    }

    /// `metadata.json` bytes.
    #[must_use]
    pub fn metadata(&self) -> &'a [u8] {
        self.member(Member::Metadata)
    }
    /// `dict.da` (prefix-dictionary trie) bytes.
    #[must_use]
    pub fn prefix_da(&self) -> &'a [u8] {
        self.member(Member::PrefixDa)
    }
    /// `dict.vals` bytes.
    #[must_use]
    pub fn prefix_vals(&self) -> &'a [u8] {
        self.member(Member::PrefixVals)
    }
    /// `dict.wordsidx` bytes.
    #[must_use]
    pub fn prefix_words_idx(&self) -> &'a [u8] {
        self.member(Member::PrefixWordsIdx)
    }
    /// `dict.words` bytes.
    #[must_use]
    pub fn prefix_words(&self) -> &'a [u8] {
        self.member(Member::PrefixWords)
    }
    /// `matrix.mtx` (connection-cost matrix) bytes.
    #[must_use]
    pub fn connection_matrix(&self) -> &'a [u8] {
        self.member(Member::ConnectionMatrix)
    }
    /// `char_def.bin` bytes.
    #[must_use]
    pub fn char_def(&self) -> &'a [u8] {
        self.member(Member::CharDef)
    }
    /// `unk.bin` (unknown-word dictionary) bytes.
    #[must_use]
    pub fn unknown(&self) -> &'a [u8] {
        self.member(Member::Unknown)
    }
}

/// Read a little-endian `u16` at `off`, or `None` if it runs past `data`.
fn read_u16(data: &[u8], off: usize) -> Option<u16> {
    let bytes = data.get(off..off.checked_add(2)?)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Read a little-endian `u32` at `off`, or `None` if it runs past `data`.
fn read_u32(data: &[u8], off: usize) -> Option<u32> {
    let bytes = data.get(off..off.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Parse `data` as a [`DictContainer`], borrowing its member ranges.
///
/// Panic-free on arbitrary input: every read is bounds-checked and every member range is validated
/// with checked arithmetic, so a truncated or hostile blob yields a [`ContainerError`], never a
/// panic or an attacker-sized allocation.
///
/// # Errors
///
/// Returns [`ContainerError`] for a short blob, wrong magic/version, a member count other than
/// [`MEMBER_COUNT`], or a member range that overflows or runs past the end of `data`.
pub fn parse(data: &[u8]) -> Result<DictContainer<'_>, ContainerError> {
    if data.len() < HEADER_LEN {
        return Err(ContainerError::Truncated {
            need: HEADER_LEN,
            got: data.len(),
        });
    }
    // The length check above guarantees the whole header (offsets 0..HEADER_LEN) is present, so the
    // header reads below cannot return `None`; they stay `get`-based regardless, to keep the parser
    // uniformly panic-free.
    if data.get(0..8) != Some(MAGIC.as_slice()) {
        return Err(ContainerError::BadMagic);
    }
    let truncated = || ContainerError::Truncated {
        need: HEADER_LEN,
        got: data.len(),
    };
    let version = read_u16(data, 8).ok_or_else(truncated)?;
    if version != VERSION {
        return Err(ContainerError::UnsupportedVersion(version));
    }
    let count = read_u16(data, 10).ok_or_else(truncated)?;
    if count as usize != MEMBER_COUNT {
        return Err(ContainerError::BadMemberCount(count));
    }

    let mut members: [&[u8]; MEMBER_COUNT] = [&[]; MEMBER_COUNT];
    for (i, slot) in members.iter_mut().enumerate() {
        // Each table entry is 8 bytes (u32 offset + u32 len) starting at offset 12.
        let entry = 12 + i * 8;
        let offset = read_u32(data, entry).ok_or_else(truncated)? as usize;
        let len = read_u32(data, entry + 4).ok_or_else(truncated)? as usize;
        let end = offset
            .checked_add(len)
            .ok_or(ContainerError::MemberRangeOverflow { index: i })?;
        *slot = data.get(offset..end).ok_or(ContainerError::Truncated {
            need: end,
            got: data.len(),
        })?;
    }
    Ok(DictContainer { members })
}

/// Build a container blob from the [`MEMBER_COUNT`] member byte ranges, in [`Member`] order.
///
/// The inverse of [`parse`]: `parse(&encode(m)?)` yields a container whose members equal `m`.
///
/// # Errors
///
/// Returns [`ContainerError::EncodeOverflow`] if the members are collectively large enough that a
/// member's start offset would not fit in a `u32` (the container then could not be parsed back).
pub fn encode(members: &[&[u8]; MEMBER_COUNT]) -> Result<Vec<u8>, ContainerError> {
    let payload_len: usize = members.iter().map(|m| m.len()).sum();
    let mut out = Vec::with_capacity(HEADER_LEN + payload_len);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    // `MEMBER_COUNT` is 8, well within u16.
    out.extend_from_slice(&(MEMBER_COUNT as u16).to_le_bytes());

    let mut offset = u32::try_from(HEADER_LEN).map_err(|_| ContainerError::EncodeOverflow)?;
    for m in members {
        let len = u32::try_from(m.len()).map_err(|_| ContainerError::EncodeOverflow)?;
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        offset = offset
            .checked_add(len)
            .ok_or(ContainerError::EncodeOverflow)?;
    }
    for m in members {
        out.extend_from_slice(m);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Eight distinguishable dummy members (different bytes + lengths, including an empty one).
    fn sample() -> [Vec<u8>; MEMBER_COUNT] {
        [
            b"{\"metadata\":1}".to_vec(),
            vec![0xDA; 5],
            vec![0x01, 0x02, 0x03],
            vec![0x10, 0x11],
            vec![0x20],
            vec![0x00, 0xFF, 0x00, 0xFF], // matrix: even length on purpose
            vec![0xC0; 7],
            Vec::new(), // an empty member must round-trip
        ]
    }

    fn refs(members: &[Vec<u8>; MEMBER_COUNT]) -> [&[u8]; MEMBER_COUNT] {
        std::array::from_fn(|i| members[i].as_slice())
    }

    #[test]
    fn encode_then_parse_round_trips_every_member() {
        let members = sample();
        let blob = encode(&refs(&members)).unwrap();
        let parsed = parse(&blob).unwrap();
        for (i, expected) in members.iter().enumerate() {
            assert_eq!(
                parsed.members[i], *expected,
                "member {i} did not round-trip"
            );
        }
        // The named accessors agree with positional order.
        assert_eq!(parsed.metadata(), members[0]);
        assert_eq!(parsed.prefix_da(), members[1]);
        assert_eq!(parsed.unknown(), members[7]);
        assert_eq!(parsed.connection_matrix(), members[5]);
    }

    #[test]
    fn parse_rejects_a_short_blob_without_panicking() {
        let members = sample();
        let blob = encode(&refs(&members)).unwrap();
        // Every truncation of a valid blob is an Err, never a panic.
        for n in 0..blob.len() {
            let err = parse(&blob[..n]).unwrap_err();
            // A prefix shorter than the header is Truncated; once the header is present but a
            // member range points past the (now shorter) end, it is Truncated too.
            assert!(
                matches!(
                    err,
                    ContainerError::Truncated { .. } | ContainerError::BadMagic
                ),
                "n={n} gave {err:?}"
            );
        }
        // The empty blob specifically.
        assert!(matches!(
            parse(&[]).unwrap_err(),
            ContainerError::Truncated { .. }
        ));
    }

    #[test]
    fn parse_rejects_bad_magic() {
        let members = sample();
        let mut blob = encode(&refs(&members)).unwrap();
        blob[0] ^= 0xFF;
        assert_eq!(parse(&blob).unwrap_err(), ContainerError::BadMagic);
    }

    #[test]
    fn parse_rejects_unsupported_version() {
        let members = sample();
        let mut blob = encode(&refs(&members)).unwrap();
        blob[8] = 2; // version low byte
        blob[9] = 0;
        assert_eq!(
            parse(&blob).unwrap_err(),
            ContainerError::UnsupportedVersion(2)
        );
    }

    #[test]
    fn parse_rejects_a_wrong_member_count() {
        let members = sample();
        for bad in [0u16, 7, 9, 65535] {
            let mut blob = encode(&refs(&members)).unwrap();
            blob[10] = (bad & 0xFF) as u8;
            blob[11] = (bad >> 8) as u8;
            assert_eq!(
                parse(&blob).unwrap_err(),
                ContainerError::BadMemberCount(bad),
                "count={bad}"
            );
        }
    }

    #[test]
    fn parse_rejects_a_member_offset_len_overflow() {
        let members = sample();
        let mut blob = encode(&refs(&members)).unwrap();
        // Member 0's table entry is at offset 12: set offset=u32::MAX, len=2 so offset+len overflows
        // usize on 32-bit and, on 64-bit, lands far past the end → either MemberRangeOverflow or
        // Truncated, and crucially NOT a panic or a giant allocation.
        blob[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        blob[16..20].copy_from_slice(&2u32.to_le_bytes());
        let err = parse(&blob).unwrap_err();
        assert!(
            matches!(
                err,
                ContainerError::MemberRangeOverflow { index: 0 } | ContainerError::Truncated { .. }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn parse_rejects_a_member_pointing_past_the_end() {
        let members = sample();
        let mut blob = encode(&refs(&members)).unwrap();
        // Member 7 (last): make its len reach one past the blob end.
        let entry = 12 + 7 * 8;
        let offset = u32::from_le_bytes(blob[entry..entry + 4].try_into().unwrap());
        let too_long = (blob.len() as u32 - offset) + 1;
        blob[entry + 4..entry + 8].copy_from_slice(&too_long.to_le_bytes());
        assert!(matches!(
            parse(&blob).unwrap_err(),
            ContainerError::Truncated { .. }
        ));
    }

    #[test]
    fn parse_never_panics_on_mutated_bytes() {
        // Defense-in-depth fuzz-lite: walk a valid blob, flip each byte through several patterns and
        // also truncate, asserting parse returns Ok or Err but never panics. Deterministic (no rng).
        let members = sample();
        let blob = encode(&refs(&members)).unwrap();
        for i in 0..blob.len() {
            for pat in [0x00u8, 0x01, 0x7F, 0x80, 0xFF] {
                let mut m = blob.clone();
                m[i] ^= pat;
                let _ = parse(&m); // must not panic
                let _ = parse(&m[..i]); // truncations too
            }
        }
    }

    #[test]
    fn empty_member_round_trips_as_an_empty_slice() {
        let members = sample();
        let blob = encode(&refs(&members)).unwrap();
        let parsed = parse(&blob).unwrap();
        assert!(
            parsed.unknown().is_empty(),
            "member 7 is empty by construction"
        );
    }

    #[test]
    fn each_error_variant_renders_a_distinct_message() {
        assert!(
            ContainerError::Truncated { need: 76, got: 3 }
                .to_string()
                .contains("truncated")
        );
        assert!(ContainerError::BadMagic.to_string().contains("magic"));
        assert!(
            ContainerError::UnsupportedVersion(2)
                .to_string()
                .contains("version 2")
        );
        assert!(ContainerError::BadMemberCount(7).to_string().contains('7'));
        assert!(
            ContainerError::MemberRangeOverflow { index: 3 }
                .to_string()
                .contains("member 3")
        );
        assert!(
            ContainerError::EncodeOverflow
                .to_string()
                .contains("too large")
        );
    }
}
