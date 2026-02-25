//! TsuzuLint Plugin Registry and Manifest Management.

pub mod cache;
pub mod downloader;
pub mod error;
pub mod fetcher;
pub use tsuzulint_manifest as manifest;
pub mod resolver;
pub mod security;
pub mod spec;

pub use downloader::{DownloadError, DownloadResult, WasmDownloader};
pub use error::FetchError;
pub use fetcher::{ManifestFetcher, PluginSource};
pub use manifest::{ExternalRuleManifest, HashVerifier, IntegrityError, ManifestError};
pub use resolver::{PluginResolver, ResolveError, ResolvedPlugin};
pub use security::{SecurityError, validate_local_wasm_path, validate_url};
pub use spec::{ParseError, PluginSpec};
