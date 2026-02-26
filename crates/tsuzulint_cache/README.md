# tsuzulint_cache

A crate that provides a file-level caching system. Implements incremental caching using BLAKE3 hashing.

## Overview

`tsuzulint_cache` provides a **file-level caching system** for the TsuzuLint project.

### Key Responsibilities

- **Skip re-linting unchanged files**: Compare content hashes to skip parsing for unchanged files
- **Invalidate cache on configuration changes**: Automatically invalidate relevant caches when rule settings change
- **Track rule versions**: Invalidate cache when WASM rule versions change
- **Incremental updates**: Support block-level differential caching

## Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                      CacheManager                            │
├─────────────────────────────────────────────────────────────┤
│  entries: HashMap<String, CacheEntry>                       │
│  cache_dir: PathBuf                                          │
│  enabled: bool                                               │
├─────────────────────────────────────────────────────────────┤
│  ┌───────────────┐    ┌──────────────────┐                  │
│  │  load/save    │◄──►│  cache.rkyv      │  (Disk)         │
│  │  (rkyv)       │    │  Zero-Copy       │                  │
│  └───────────────┘    └──────────────────┘                  │
├─────────────────────────────────────────────────────────────┤
│  ┌───────────────────────────────────────────────────────┐  │
│  │                    CacheEntry                          │  │
│  │  - content_hash: [u8; 32] (BLAKE3)                    │  │
│  │  - config_hash: [u8; 32]                              │  │
│  │  - rule_versions: HashMap<String, String>             │  │
│  │  - diagnostics: Vec<Diagnostic>                       │  │
│  │  - blocks: Vec<BlockCacheEntry>                       │  │
│  │    └─ hash: [u8; 32] (BLAKE3)                        │  │
│  │    └─ span: Span                                      │  │
│  │    └─ diagnostics: Vec<Diagnostic>                   │  │
│  │  - created_at: u64                                    │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## Cache Key Generation (BLAKE3)

### Implementation

```rust
pub fn hash_content(content: &str) -> BlockHash {
    blake3::hash(content.as_bytes()).into()
}
```

### Why BLAKE3?

- **High Performance**: Faster than SHA-256 (with SIMD optimization support)
- **Security**: Cryptographically secure hash function
- **Consistency**: Always generates the same 256-bit (32-byte) hash from identical input

### Cache Key Components

1. **Content Hash**: BLAKE3 hash of file contents (`[u8; 32]`)
2. **Config Hash**: Hash of lint configuration (`[u8; 32]`)
3. **Rule Versions**: Manages rule names and versions in `HashMap<String, String>` format
4. **Block Hash**: 32-byte array (`[u8; 32]`) for block-level differential detection

## Cache Invalidation Strategy

### Validation Logic

```rust
pub fn is_valid(
    &self,
    content_hash: &BlockHash,
    config_hash: &BlockHash,
    rule_versions: &HashMap<String, String>,
) -> bool {
    self.content_hash == *content_hash
        && self.config_hash == *config_hash
        && self.rule_versions == *rule_versions
}
```

### Invalidation Triggers

| Condition | Result |
| --------- | ------ |
| File content changed | Cache invalidated |
| Configuration file changed | Cache invalidated |
| Rule version changed | Cache invalidated |
| Number of rules changed | Cache invalidated |
| Rule name changed | Cache invalidated |

## Incremental Block Cache

The `reconcile_blocks()` method enables block-level differential reuse:

```rust
pub fn reconcile_blocks(
    &self,
    path: &Path,
    current_blocks: &[BlockCacheEntry],
    config_hash: &BlockHash,
    rule_versions: &HashMap<String, String>,
) -> (Vec<Diagnostic>, Vec<bool>)
```

**How It Works:**

1. Map cached blocks by hash value
2. Reuse diagnostic results when current block hash matches
3. **Position Shift Correction**: Correct diagnostic Spans when blocks have moved
4. Use `find_best_match()` to select the candidate with the closest position (minimizing position shift)

## Cache Storage Format

### Zero-Copy Deserialization (rkyv)

```rust
// Save
let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&self.entries)?;
fs::write(&cache_file, bytes)?;

// Load
let content = fs::read(&cache_file)?;
let entries: HashMap<String, CacheEntry> =
    rkyv::from_bytes::<_, rkyv::rancor::Error>(&content)?;
```

### File Structure

```text
<cache_dir>/
└── cache.rkyv    # rkyv format binary file
```

### Benefits of rkyv

| Feature | Description |
| ------- | ----------- |
| **No Parsing Required** | Direct access from byte sequence |
| **Memory Efficient** | Data access without additional allocations |
| **Fast Startup** | Minimal overhead when loading cache |
| **Archive Types** | Auto-generated via `rkyv::Archive` derive |

## Usage Examples

### Basic Usage

```rust
use tsuzulint_cache::{CacheManager, CacheEntry};
use std::collections::HashMap;

// Create cache manager
let mut manager = CacheManager::new(".cache/tsuzulint")?;

// Calculate content hash
let content_hash = CacheManager::hash_content("source text");

// Check cache
if let Some(entry) = manager.get("path/to/file.md") {
    if entry.is_valid(&content_hash, &config_hash, &rule_versions) {
        // Cache hit - reuse diagnostic results
        return Ok(entry.diagnostics);
    }
}

// Cache miss - run lint, then cache results
use std::time::{SystemTime, UNIX_EPOCH};
let created_at = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()
    .as_secs();

let entry = CacheEntry {
    content_hash,
    config_hash,
    rule_versions,
    diagnostics,
    blocks: vec![],
    created_at,
};
manager.set("path/to/file.md", entry);

// Save to disk
manager.save()?;
```

### Incremental Block Cache Usage

```rust
// Utilize block-level cache
let (cached_diagnostics, block_validity) = manager.reconcile_blocks(
    &path,
    &current_blocks,
    &config_hash,
    &rule_versions,
);

// Re-lint only changed blocks
for (i, is_valid) in block_validity.iter().enumerate() {
    if !is_valid {
        // Re-lint this block
    }
}
```

## Public API

```rust
pub use entry::CacheEntry;
pub use error::CacheError;
pub use manager::CacheManager;
```

### CacheManager Main Methods

| Method | Description |
| ------ | ----------- |
| `new(cache_dir)` | Create cache manager |
| `enable()` / `disable()` | Enable/disable caching |
| `get(path)` | Get cache entry |
| `set(path, entry)` | Store cache entry |
| `is_valid(...)` | Check cache validity |
| `reconcile_blocks(...)` | Block-level differential reuse |
| `load()` | Load cache from disk |
| `save()` | Save cache to disk |
| `clear()` | Clear all cache |
| `hash_content(content)` | Generate BLAKE3 hash of content |

## Feature Flags

```toml
[features]
default = ["native"]
native = ["tsuzulint_plugin/native"]
browser = ["tsuzulint_plugin/browser"]
```

- **native**: For native environments (CLI usage)
- **browser**: For browser WASM environments (used by tsuzulint_wasm)

## Dependencies

| Crate | Usage |
| ----- | ----- |
| **blake3** | Fast cryptographic hash function. Generates content/block hashes |
| **rkyv** | Zero-copy serialization. Cache persistence |
| **serde / serde_json** | JSON serialization |
| **thiserror** | Error type definitions |
| **tracing** | Logging output |
| **tsuzulint_plugin** | Shared `Diagnostic` type |
| **tsuzulint_ast** | Shared `Span` type |
