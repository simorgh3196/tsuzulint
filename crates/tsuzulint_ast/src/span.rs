//! Span and position types for source locations.
//!
//! These types represent positions within source text, compatible with
//! textlint's TxtAST specification.

use serde::{Deserialize, Serialize};

/// A position in source text.
///
/// Uses 1-indexed lines and 0-indexed columns for compatibility with
/// textlint and JavaScript AST conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Position {
    /// Line number (1-indexed).
    pub line: u32,
    /// Column number (0-indexed).
    pub column: u32,
}

impl Position {
    /// Creates a new position.
    #[inline]
    pub const fn new(line: u32, column: u32) -> Self {
        Self { line, column }
    }
}

/// A span representing a range in source text.
///
/// Uses byte offsets (0-indexed) for efficient slicing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Span {
    /// Start byte offset (0-indexed, inclusive).
    pub start: u32,
    /// End byte offset (0-indexed, exclusive).
    pub end: u32,
}

impl Span {
    /// Creates a new span.
    #[inline]
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Returns the length of the span in bytes.
    #[inline]
    pub const fn len(&self) -> u32 {
        self.end - self.start
    }

    /// Returns true if the span is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Returns true if this span contains the given offset.
    #[inline]
    pub const fn contains(&self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }

    /// Merges two spans into one that covers both.
    #[inline]
    pub const fn merge(&self, other: &Span) -> Span {
        Span {
            start: if self.start < other.start {
                self.start
            } else {
                other.start
            },
            end: if self.end > other.end {
                self.end
            } else {
                other.end
            },
        }
    }
}

/// Location information combining start and end positions.
///
/// This is used for serialization to match textlint's `loc` format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(
    feature = "rkyv",
    derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)
)]
pub struct Location {
    /// Start position.
    pub start: Position,
    /// End position.
    pub end: Position,
}

impl Location {
    /// Creates a new location.
    #[inline]
    pub const fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position() {
        let pos = Position::new(1, 0);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.column, 0);
    }

    #[test]
    fn test_span() {
        let span = Span::new(10, 20);
        assert_eq!(span.len(), 10);
        assert!(!span.is_empty());
        assert!(span.contains(15));
        assert!(!span.contains(5));
        assert!(!span.contains(20));
    }

    #[test]
    fn test_span_merge() {
        let span1 = Span::new(10, 20);
        let span2 = Span::new(15, 30);
        let merged = span1.merge(&span2);
        assert_eq!(merged.start, 10);
        assert_eq!(merged.end, 30);
    }

    #[test]
    fn test_empty_span() {
        let span = Span::new(5, 5);
        assert!(span.is_empty());
        assert_eq!(span.len(), 0);
    }

    #[test]
    fn test_location() {
        let start = Position::new(1, 0);
        let end = Position::new(1, 10);
        let loc = Location::new(start, end);

        assert_eq!(loc.start.line, 1);
        assert_eq!(loc.start.column, 0);
        assert_eq!(loc.end.line, 1);
        assert_eq!(loc.end.column, 10);
    }

    #[test]
    fn test_span_contains_start() {
        let span = Span::new(10, 20);
        assert!(span.contains(10)); // Start is inclusive
    }

    #[test]
    fn test_span_contains_end_exclusive() {
        let span = Span::new(10, 20);
        assert!(!span.contains(20)); // End is exclusive
    }

    #[test]
    fn test_span_merge_non_overlapping() {
        let span1 = Span::new(0, 5);
        let span2 = Span::new(10, 15);
        let merged = span1.merge(&span2);

        assert_eq!(merged.start, 0);
        assert_eq!(merged.end, 15);
    }

    #[test]
    fn test_span_merge_containing() {
        let outer = Span::new(0, 100);
        let inner = Span::new(20, 30);
        let merged = outer.merge(&inner);

        assert_eq!(merged.start, 0);
        assert_eq!(merged.end, 100);
    }

    #[test]
    fn test_span_merge_same_span() {
        let span = Span::new(5, 10);
        let merged = span.merge(&span);

        assert_eq!(merged.start, 5);
        assert_eq!(merged.end, 10);
    }

    #[test]
    fn test_span_merge_reversed_order() {
        let span1 = Span::new(20, 30);
        let span2 = Span::new(10, 15);
        let merged = span1.merge(&span2);

        assert_eq!(merged.start, 10);
        assert_eq!(merged.end, 30);
    }

    #[test]
    fn test_position_equality() {
        let pos1 = Position::new(5, 10);
        let pos2 = Position::new(5, 10);
        let pos3 = Position::new(5, 11);

        assert_eq!(pos1, pos2);
        assert_ne!(pos1, pos3);
    }

    #[test]
    fn test_span_equality() {
        let span1 = Span::new(0, 10);
        let span2 = Span::new(0, 10);
        let span3 = Span::new(0, 11);

        assert_eq!(span1, span2);
        assert_ne!(span1, span3);
    }

    #[test]
    fn test_location_equality() {
        let loc1 = Location::new(Position::new(1, 0), Position::new(1, 5));
        let loc2 = Location::new(Position::new(1, 0), Position::new(1, 5));
        let loc3 = Location::new(Position::new(1, 0), Position::new(2, 0));

        assert_eq!(loc1, loc2);
        assert_ne!(loc1, loc3);
    }

    #[test]
    fn test_span_serialization() {
        let span = Span::new(10, 20);
        let json = serde_json::to_string(&span).unwrap();
        assert!(json.contains("10"));
        assert!(json.contains("20"));
    }

    #[test]
    fn test_span_deserialization() {
        let json = r#"{"start": 5, "end": 15}"#;
        let span: Span = serde_json::from_str(json).unwrap();
        assert_eq!(span.start, 5);
        assert_eq!(span.end, 15);
    }

    #[test]
    fn test_position_serialization() {
        let pos = Position::new(10, 5);
        let json = serde_json::to_string(&pos).unwrap();
        assert!(json.contains("10"));
        assert!(json.contains("5"));
    }

    #[test]
    fn test_location_serialization() {
        let loc = Location::new(Position::new(1, 0), Position::new(1, 10));
        let json = serde_json::to_string(&loc).unwrap();
        assert!(json.contains("start"));
        assert!(json.contains("end"));
    }

    #[test]
    fn test_empty_span_contains() {
        let span = Span::new(5, 5);
        // Empty span contains nothing
        assert!(!span.contains(5));
        assert!(!span.contains(4));
        assert!(!span.contains(6));
    }

    #[test]
    fn test_span_at_zero() {
        let span = Span::new(0, 10);
        assert!(span.contains(0));
        assert_eq!(span.len(), 10);
    }

    #[test]
    fn test_single_byte_span() {
        let span = Span::new(5, 6);
        assert_eq!(span.len(), 1);
        assert!(!span.is_empty());
        assert!(span.contains(5));
        assert!(!span.contains(6));
    }
}
