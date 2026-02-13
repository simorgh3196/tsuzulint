use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower_lsp::LspService;
use tsuzulint_lsp::Backend;

#[tokio::test]
async fn test_did_change_validation_frequency() {
    // Pipe for communicating with the server
    let (client_read, server_write) = tokio::io::duplex(4096);
    let (server_read, client_write) = tokio::io::duplex(4096);

    let (service, socket) = LspService::new(Backend::new);

    // Start server in background
    let server_handle = tokio::spawn(async move {
        tower_lsp::Server::new(server_read, server_write, socket)
            .serve(service)
            .await;
    });

    // Helper to read LSP messages
    let mut reader = tokio::io::BufReader::new(client_read);
    let mut writer = client_write;

    // Background task to read messages from server and prevent deadlock
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(msg) = recv_msg(&mut reader).await {
            if tx.send(msg).is_err() {
                break; // Receiver dropped, stop reading
            }
        }
    });

    // Setup paths
    let temp_dir = tempfile::tempdir().unwrap();
    let root_path = temp_dir.path();
    let file_path = root_path.join("test.md");
    let root_uri = tower_lsp::lsp_types::Url::from_file_path(root_path).unwrap();
    let file_uri = tower_lsp::lsp_types::Url::from_file_path(file_path).unwrap();

    // 1. Initialize
    let init_req = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"rootUri":"{}","capabilities":{{}}}}}}"#,
        root_uri
    );
    send_msg(&mut writer, &init_req).await;

    // Read initialize response
    let _resp = rx.recv().await.unwrap();
    // println!("Init Resp: {}", _resp);

    // 2. Initialized
    let initialized_notif = r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#;
    send_msg(&mut writer, initialized_notif).await;

    // 3. DidOpen
    let did_open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"{}","languageId":"markdown","version":0,"text":"start"}}}}}}"#,
        file_uri
    );
    send_msg(&mut writer, &did_open).await;

    // Expect publishDiagnostics from didOpen
    // We expect it relatively quickly, but allow more time for slow CI/Windows
    let mut got_diagnostics = false;
    let timeout = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            msg_opt = rx.recv() => {
                if let Some(msg) = msg_opt {
                    if msg.contains("publishDiagnostics") {
                        got_diagnostics = true;
                        break;
                    }
                } else {
                    break;
                }
            }
            _ = &mut timeout => break,
        }
    }

    assert!(got_diagnostics, "Expected diagnostics after open");

    // 4. Send multiple DidChange fast
    // Because we are reading in background, server won't block on writing.
    for i in 1..=5 {
        let did_change = format!(
            r#"{{"jsonrpc":"2.0","method":"textDocument/didChange","params":{{"textDocument":{{"uri":"{}","version":{}}},"contentChanges":[{{"text":"change {}"}}]}}}}"#,
            file_uri, i, i
        );
        send_msg(&mut writer, &did_change).await;
        // Small delay to simulate typing but still fast enough to trigger debounce
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // 5. Wait for debounce (300ms) + some buffer
    // We expect at most 1 or 2 messages.
    // Allow generous timeout for CI
    let timeout = tokio::time::sleep(Duration::from_secs(2));
    tokio::pin!(timeout);

    let mut diagnostics_count = 0;
    loop {
        tokio::select! {
            msg_opt = rx.recv() => {
                match msg_opt {
                    Some(msg) => {
                        if msg.contains("publishDiagnostics") {
                            diagnostics_count += 1;
                        }
                    }
                    None => break, // Stream closed
                }
            }
            _ = &mut timeout => {
                break;
            }
        }
    }

    println!("Total additional diagnostics: {}", diagnostics_count);

    // We sent 5 changes.
    // If no debounce: we might get 5.
    // With debounce: we expect 1 (for the last one), maybe 2 if timing is loose.
    assert!(
        diagnostics_count <= 2,
        "Debounce failed: got {} diagnostics",
        diagnostics_count
    );
    assert!(
        diagnostics_count > 0,
        "Expected at least 1 diagnostic (final state)"
    );

    // Ensure server didn't crash
    drop(writer); // Close connection to let server exit
    let _ = tokio::time::timeout(Duration::from_secs(1), server_handle).await;
}

async fn send_msg<W: AsyncWriteExt + Unpin>(writer: &mut W, msg: &str) {
    let content = format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
    writer.write_all(content.as_bytes()).await.unwrap();
    writer.flush().await.unwrap();
}

async fn recv_msg<R: AsyncReadExt + Unpin>(reader: &mut R) -> Option<String> {
    // Simple LSP parser: read headers until \r\n\r\n, parse Content-Length, read body
    let mut buffer = Vec::new();
    let mut content_length = 0;

    // Read headers
    loop {
        let byte = reader.read_u8().await.ok()?;
        buffer.push(byte);
        if buffer.ends_with(b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&buffer);
            for line in headers.lines() {
                if line.to_lowercase().starts_with("content-length:") {
                    let parts: Vec<&str> = line.split(':').collect();
                    if parts.len() == 2 {
                        content_length = parts[1].trim().parse().unwrap_or_else(|e| {
                            panic!("Failed to parse Content-Length: {e}, header: {line}")
                        });
                    }
                }
            }
            break;
        }
    }

    if content_length == 0 {
        return None;
    }

    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).await.ok()?;

    Some(String::from_utf8(body).unwrap())
}
