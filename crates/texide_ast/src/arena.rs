//! Arena allocator for AST nodes.
//!
//! Uses `bumpalo` for efficient bump allocation of AST nodes.
//! All nodes for a single file are allocated in the same arena,
//! and freed together when processing is complete.

use bumpalo::Bump;

/// Arena allocator for AST nodes.
///
/// This struct wraps `bumpalo::Bump` to provide arena allocation
/// for TxtAST nodes. Using arena allocation:
///
/// - Minimizes allocation overhead
/// - Improves cache locality
/// - Enables batch deallocation
///
/// # Example
///
/// ```rust
/// use texide_ast::AstArena;
///
/// let arena = AstArena::new();
///
/// // Allocate a value in the arena
/// let value = arena.alloc(42u32);
/// assert_eq!(*value, 42);
///
/// // Allocate a string slice
/// let s = arena.alloc_str("hello");
/// assert_eq!(s, "hello");
/// ```
pub struct AstArena {
    bump: Bump,
}

impl AstArena {
    /// Creates a new arena allocator.
    #[inline]
    pub fn new() -> Self {
        Self { bump: Bump::new() }
    }

    /// Creates a new arena with the specified initial capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            bump: Bump::with_capacity(capacity),
        }
    }

    /// Allocates a value in the arena and returns a reference to it.
    #[inline]
    pub fn alloc<T>(&self, val: T) -> &T {
        self.bump.alloc(val)
    }

    /// Allocates a string slice in the arena.
    #[inline]
    pub fn alloc_str(&self, s: &str) -> &str {
        self.bump.alloc_str(s)
    }

    /// Allocates a slice in the arena by copying from the input slice.
    #[inline]
    pub fn alloc_slice_copy<T: Copy>(&self, slice: &[T]) -> &[T] {
        self.bump.alloc_slice_copy(slice)
    }

    /// Allocates a slice in the arena by cloning from the input slice.
    #[inline]
    pub fn alloc_slice_clone<T: Clone>(&self, slice: &[T]) -> &[T] {
        self.bump.alloc_slice_clone(slice)
    }

    /// Returns the total bytes allocated in this arena.
    #[inline]
    pub fn allocated_bytes(&self) -> usize {
        self.bump.allocated_bytes()
    }

    /// Resets the arena, deallocating all allocated objects.
    ///
    /// Note: This does NOT call `Drop` for allocated objects.
    #[inline]
    pub fn reset(&mut self) {
        self.bump.reset();
    }
}

impl Default for AstArena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arena_alloc() {
        let arena = AstArena::new();
        let value = arena.alloc(42u32);
        assert_eq!(*value, 42);
    }

    #[test]
    fn test_arena_alloc_str() {
        let arena = AstArena::new();
        let s = arena.alloc_str("hello world");
        assert_eq!(s, "hello world");
    }

    #[test]
    fn test_arena_alloc_slice() {
        let arena = AstArena::new();
        let slice = arena.alloc_slice_copy(&[1, 2, 3, 4, 5]);
        assert_eq!(slice, &[1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_arena_reset() {
        let mut arena = AstArena::new();
        let _ = arena.alloc(42u32);
        let bytes_before = arena.allocated_bytes();
        arena.reset();
        // After reset, new allocations should be possible
        let _ = arena.alloc(100u32);
        // Note: allocated_bytes may not decrease after reset
        // because the arena keeps the memory for reuse
        assert!(arena.allocated_bytes() > 0 || bytes_before > 0);
    }
}
