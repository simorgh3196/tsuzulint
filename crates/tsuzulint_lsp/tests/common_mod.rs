use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn send_msg<W: AsyncWriteExt + Unpin>(writer: &mut W, msg: &str) {
    let content = format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
    writer.write_all(content.as_bytes()).await.unwrap();
    writer.flush().await.unwrap();
}

pub async fn recv_msg<R: AsyncReadExt + Unpin>(reader: &mut R) -> Option<String> {
    let mut buffer = Vec::new();
    let mut content_length = 0;

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
