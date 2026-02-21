use std::time::Instant;
use tsuzulint_registry::downloader::WasmDownloader;
use tsuzulint_registry::manifest::{Artifacts, ExternalRuleManifest, IsolationLevel, RuleMetadata};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::main]
async fn main() {
    let mock_server = MockServer::start().await;
    let file_size_mb = 10;
    let file_size_bytes = file_size_mb * 1024 * 1024;

    // Create a large buffer to serve
    // We create it once and share it to avoid allocation overhead in the benchmark loop logic
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

    // Use a large max size to allow download
    let downloader =
        WasmDownloader::with_max_size((file_size_mb + 10) as u64 * 1024 * 1024).allow_local(true);

    println!("Starting download benchmark ({} MB file)...", file_size_mb);

    // Warm-up run
    print!("Warm-up... ");
    let _ = downloader
        .download(&manifest)
        .await
        .expect("Download failed");
    println!("Done.");

    let iterations = 10;
    let mut durations = Vec::with_capacity(iterations);

    for i in 1..=iterations {
        let start = Instant::now();
        let _ = downloader
            .download(&manifest)
            .await
            .expect("Download failed");
        let duration = start.elapsed();
        durations.push(duration);
        println!("Iteration {}: {:.2?}", i, duration);
    }

    let total_duration: std::time::Duration = durations.iter().sum();
    let avg_duration = total_duration / iterations as u32;

    // Calculate Standard Deviation
    let avg_micros = avg_duration.as_micros() as f64;
    let variance = durations
        .iter()
        .map(|d| {
            let diff = d.as_micros() as f64 - avg_micros;
            diff * diff
        })
        .sum::<f64>()
        / iterations as f64;
    let std_dev = variance.sqrt();

    let avg_throughput = file_size_mb as f64 / avg_duration.as_secs_f64();

    println!("\n--- Results ---");
    println!("Average time: {:.2?}", avg_duration);
    println!("Std Dev: {:.2} ms", std_dev / 1000.0);
    println!("Average Throughput: {:.2} MB/s", avg_throughput);
}
