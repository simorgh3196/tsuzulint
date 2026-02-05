//! TsuzuLint Plugin Registry and Manifest Management.

pub mod cache;
pub mod downloader;
pub mod error;
pub mod fetcher;
pub mod hash;
pub mod manifest;
pub mod resolver;

pub use downloader::{DownloadError, DownloadResult, WasmDownloader};
pub use error::FetchError;
pub use fetcher::{ManifestFetcher, PluginSource};
pub use hash::{HashError, HashVerifier};
pub use manifest::{ExternalRuleManifest, ManifestError};
pub use resolver::{ParseError, PluginResolver, PluginSpec, ResolveError, ResolvedPlugin};
