//! Span and position types for source locations.
//!
//! These types represent positions within source text, compatible with
//! textlint's TxtAST specification.

use serde::{Deserialize, Serialize};

/// A position in source text.
///
/// Uses 1-indexed lines and 0-indexed columns for compatibility with
/// textlint and JavaScript AST conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
}
