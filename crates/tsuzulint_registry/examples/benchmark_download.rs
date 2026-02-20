use std::time::Instant;
use tsuzulint_registry::downloader::WasmDownloader;
use tsuzulint_registry::manifest::{Artifacts, ExternalRuleManifest, IsolationLevel, RuleMetadata};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::main]
async fn main() {
    let mock_server = MockServer::start().await;
    let file_size_mb = 50;
    let file_size_bytes = file_size_mb * 1024 * 1024;

    // Create a large buffer to serve
    let body = vec![b'x'; file_size_bytes];

    Mock::given(method("GET"))
        .and(path("/large.wasm"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
        .mount(&mock_server)
        .await;

    let manifest = ExternalRuleManifest {
        rule: RuleMetadata {
            name: "benchmark-rule".to_string(),
            version: "1.0.0".to_string(),
            description: None,
            repository: None,
            license: None,
            authors: vec![],
            keywords: vec![],
            fixable: false,
            node_types: vec![],
            isolation_level: IsolationLevel::Global,
        },
        artifacts: Artifacts {
            wasm: format!("{}/large.wasm", mock_server.uri()),
            sha256: "ignored".to_string(),
        },
        permissions: None,
        tsuzulint: None,
        options: None,
    };

    // Use a large max size to allow 50MB download
    let downloader = WasmDownloader::with_max_size((file_size_mb + 10) as u64 * 1024 * 1024);

    println!("Starting download benchmark ({} MB)...", file_size_mb);

    let start = Instant::now();
    let result = downloader
        .download(&manifest)
        .await
        .expect("Download failed");
    let duration = start.elapsed();

    println!("Download completed in {:.2?}", duration);
    println!(
        "Throughput: {:.2} MB/s",
        file_size_mb as f64 / duration.as_secs_f64()
    );
    println!("Downloaded size: {} bytes", result.bytes.len());
}
