# tsuzulint_registry

A crate responsible for plugin registry and package management. Fetches and caches plugins from GitHub, URLs, and local paths.

## Overview

`tsuzulint_registry` handles **plugin registry and package management** for the TsuzuLint project.

### Key Responsibilities

1. **External Rule Plugin Retrieval**: Fetch plugin manifests from GitHub, URLs, and local paths
2. **WASM Artifact Downloads**: Safely download plugin WASM binaries
3. **Cache Management**: Cache downloaded plugins locally for efficient reuse
4. **Security Protection**: URL validation to prevent SSRF attacks, hash verification

## Architecture

```text
┌─────────────────────────────────────────────────────────────────┐
│                      PluginResolver                              │
│  (Central coordinator for plugin resolution)                     │
└─────────────────────────────────────────────────────────────────┘
          │                    │                    │
          ▼                    ▼                    ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐
│ ManifestFetcher │  │  WasmDownloader │  │   PluginCache   │
│ (Manifest Fetch)│  │(WASM Download)  │  │ (Cache Manager) │
└─────────────────┘  └─────────────────┘  └─────────────────┘
          │                    │                    │
          └────────┬───────────┘                    │
                   ▼                                ▼
┌─────────────────────────────────────┐  ┌─────────────────┐
│        SecureHttpClient             │  │  File System    │
│  (SSRF + DNS Rebinding Protection)  │  │  ~/.cache/...   │
└─────────────────────────────────────┘  └─────────────────┘
          │                    │
          ▼                    ▼
┌─────────────────┐  ┌─────────────────────────────────────────────┐
│  PluginSource   │  │         validate_url / check_ip (Security)   │
│ (Source Type)   │  │         SSRF + DNS Rebinding Protection      │
└─────────────────┘  └─────────────────────────────────────────────┘
          │
          ▼
┌─────────────────────────────────────┐
│  tsuzulint_manifest::HashVerifier   │
│  (SHA256 Integrity Verification)    │
└─────────────────────────────────────┘
```

## PluginSource (Plugin Source)

Supports 3 types of sources:

```rust
pub enum PluginSource {
    /// GitHub repository: `owner/repo` or `owner/repo@version`
    GitHub { owner: String, repo: String, version: Option<String> },
    /// Direct URL
    Url(String),
    /// Local file path
    Path(PathBuf),
}
```

### GitHub Source

- `owner/repo` → Fetch from latest release
- `owner/repo@v1.2.3` → Fetch from specific version
- URL format: `{base}/{owner}/{repo}/releases/download/v{version}/tsuzulint-rule.json`

## Resolution Flow

```text
PluginSpec Parsing
    ↓
Cache Check
    ├─ Hit → Return ResolvedPlugin
    └─ Miss → Execute Fetch
           ↓
       Manifest Fetch
           ↓
       WASM Download
           ↓
       SHA256 Hash Verification
           ↓
       Cache Save
           ↓
       Return ResolvedPlugin
```

### PluginSpec Parse Formats

```json
// String format
"owner/repo"
"owner/repo@v1.0.0"

// Object format
{"github": "owner/repo", "as": "my-rule"}
{"url": "https://example.com/manifest.json", "as": "external-rule"}
{"path": "./local/rule", "as": "local-rule"}
```

## Cache Management

### Cache Location

`~/.cache/tsuzulint/plugins/` (Unix systems)

### Directory Structure

```text
~/.cache/tsuzulint/plugins/
├── owner/
│   └── repo/
│       └── v1.0.0/
│           ├── rule.wasm
│           └── tsuzulint-rule.json
└── url/
    └── {sha256_of_url}/
        └── v1.0.0/
            ├── rule.wasm
            └── tsuzulint-rule.json
```

### Features

- Path traversal attack prevention
- Replace cached manifest's `artifacts.wasm` with local path
- URL sources use SHA256 hash of URL as key

## Security Features

### SSRF Protection (`validate_url`)

**Blocked by Default:**

- `localhost` domain
- IPv4 loopback (`127.0.0.0/8`)
- IPv4 unspecified (`0.0.0.0`)
- IPv4 private (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`)
- IPv4 link-local (`169.254.0.0/16`)
- IPv6 loopback (`::1`)
- IPv6 unspecified (`::`)
- IPv6 unique local (`fc00::/7`)
- IPv6 link-local (`fe80::/10`)
- Non-HTTP schemes (`ftp://`, `file://`, etc.)

**Allowed for Testing/Development:**

```rust
let fetcher = ManifestFetcher::new().allow_local(true);
let downloader = WasmDownloader::new().allow_local(true);
```

### DNS Rebinding Protection

When `allow_local = false`, the client:

1. Resolves DNS explicitly using `tokio::net::lookup_host`
2. Validates all resolved IPs against the security policy
3. Pins validated IPs to the HTTP client using `resolve_to_addrs`

This prevents DNS rebinding attacks where an attacker-controlled domain
initially resolves to a public IP but later resolves to a private IP.

### Hash Verification

- Uses `tsuzulint_manifest::HashVerifier` for SHA256 hash verification
- Automatic SHA256 calculation of downloaded WASM
- Comparison with manifest's `artifacts.sha256`
- Returns `IntegrityError::HashMismatch` on mismatch

### Path Traversal Protection

**Local Path Sources:**

- Absolute paths prohibited
- `..` components prohibited
- Verify normalized path is within manifest's parent directory

**Cache:**

- Validate `owner`, `repo`, `version` are single valid path components

## WasmDownloader

```rust
pub struct WasmDownloader {
    http_client: SecureHttpClient,  // Handles HTTP with SSRF protection
    max_size: u64,                   // Default: 50MB
    timeout: Duration,               // Default: 60 seconds (stored for allow_local rebuild)
}
```

**Features:**

- Size limit checking after download
- Timeout configuration
- `{version}` placeholder substitution
- Automatic hash calculation
- Uses `SecureHttpClient` for SSRF/DNS Rebinding protection

## SecureHttpClient

```rust
pub struct SecureHttpClient {
    timeout: Duration,        // Default: 10 seconds
    allow_local: bool,        // Default: false
    max_redirects: u32,       // Default: 10
}
```

**Features:**

- DNS resolution with timeout
- IP validation (SSRF protection)
- DNS pinning (DNS Rebinding protection)
- Manual redirect handling with validation per hop
- Maximum redirect limit (default: 10)

**Usage:**

```rust
use tsuzulint_registry::http_client::SecureHttpClient;
use std::time::Duration;

let client = SecureHttpClient::builder()
    .timeout(Duration::from_secs(30))
    .allow_local(true)
    .max_redirects(5)
    .build();

let content = client.fetch("https://example.com/data").await?;
```

## Usage Examples

### Basic Usage

```rust
use tsuzulint_registry::{PluginResolver, PluginSpec};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create resolver
    let resolver = PluginResolver::new()?;
    
    // Resolve plugin from GitHub
    let spec = PluginSpec::parse(&json!("simorgh3196/tsuzulint-rule-no-todo@v1.0.0"))?;
    let resolved = resolver.resolve(&spec).await?;
    
    println!("WASM path: {:?}", resolved.wasm_path);
    println!("Manifest path: {:?}", resolved.manifest_path);
    println!("Alias: {}", resolved.alias);
    
    Ok(())
}
```

### Custom Configuration

```rust
use tsuzulint_registry::downloader::WasmDownloader;
use std::time::Duration;

let downloader = WasmDownloader::with_options(
    100 * 1024 * 1024,        // 100MB max size
    Duration::from_secs(120), // 2 min timeout
);
```

### CLI Usage

```bash
# Install from GitHub
tzlint plugin install owner/repo

# Specific version
tzlint plugin install owner/repo@v1.0.0

# With alias
tzlint plugin install owner/repo --as my-rule

# From URL
tzlint plugin install --url https://example.com/rule.wasm --as external-rule

# Clear cache
tzlint plugin cache clean
```

## Module Structure

| Module | Responsibility |
| ------ | -------------- |
| `lib.rs` | Entry point, public API re-exports |
| `fetcher.rs` | Plugin manifest fetching |
| `downloader.rs` | WASM binary download |
| `http_client.rs` | Secure HTTP fetch with SSRF/DNS protection |
| `resolver.rs` | Plugin resolution integration |
| `cache.rs` | Local plugin cache |
| `security.rs` | URL security validation |
| `error.rs` | Error type definitions |

## Dependencies

| Crate | Purpose |
| ----- | ------- |
| `tsuzulint_manifest` | Plugin manifest types and `HashVerifier` for integrity checks |
| `reqwest` | HTTP client (with streaming support) |
| `futures-util` | Async streaming processing |
| `dirs` | Cache directory retrieval |
| `url` | URL parsing |
| `tokio` | Async runtime |
| `tracing` | Logging |
