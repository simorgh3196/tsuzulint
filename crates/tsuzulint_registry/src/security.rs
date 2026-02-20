use reqwest::Url;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Invalid URL scheme: {0}")]
    InvalidScheme(String),
    #[error("Access to loopback address denied: {0}")]
    LoopbackDenied(String),
    #[error("Access to private IP address denied: {0}")]
    PrivateIpDenied(String),
}

pub fn validate_url(url: &Url, allow_local: bool) -> Result<(), SecurityError> {
    if allow_local {
        return Ok(());
    }

    if url.scheme() != "http" && url.scheme() != "https" {
        return Err(SecurityError::InvalidScheme(url.scheme().to_string()));
    }

    match url.host() {
        Some(url::Host::Domain(domain)) => {
            if domain == "localhost" {
                return Err(SecurityError::LoopbackDenied(domain.to_string()));
            }
        }
        Some(url::Host::Ipv4(ipv4)) => {
            if ipv4.is_loopback() || ipv4.is_unspecified() {
                return Err(SecurityError::LoopbackDenied(ipv4.to_string()));
            }
            if ipv4.is_private() || ipv4.is_link_local() {
                return Err(SecurityError::PrivateIpDenied(ipv4.to_string()));
            }
        }
        Some(url::Host::Ipv6(ipv6)) => {
            if ipv6.is_loopback() || ipv6.is_unspecified() {
                return Err(SecurityError::LoopbackDenied(ipv6.to_string()));
            }
            // Unique local (fc00::/7)
            if (ipv6.segments()[0] & 0xfe00) == 0xfc00 || ipv6.is_unicast_link_local() {
                return Err(SecurityError::PrivateIpDenied(ipv6.to_string()));
            }
        }
        None => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_valid(url_str: &str) {
        let url = Url::parse(url_str).unwrap();
        assert!(validate_url(&url, false).is_ok(), "Expected valid: {}", url_str);
    }

    fn assert_invalid(url_str: &str) {
        let url = Url::parse(url_str).unwrap();
        assert!(validate_url(&url, false).is_err(), "Expected invalid: {}", url_str);
    }

    #[test]
    fn test_valid_public_urls() {
        assert_valid("https://example.com/rule.wasm");
        assert_valid("http://example.com/rule.wasm");
        assert_valid("https://8.8.8.8/rule.wasm");
        assert_valid("https://[2001:4860:4860::8888]/rule.wasm");
    }

    #[test]
    fn test_invalid_scheme() {
        assert_invalid("ftp://example.com/rule.wasm");
        assert_invalid("file:///etc/passwd");
        assert_invalid("gopher://example.com");
    }

    #[test]
    fn test_localhost_string() {
        assert_invalid("http://localhost/rule.wasm");
        assert_invalid("http://localhost:8080/rule.wasm");
    }

    #[test]
    fn test_loopback_ipv4() {
        assert_invalid("http://127.0.0.1/rule.wasm");
        assert_invalid("http://127.0.0.1:8080/rule.wasm");
        assert_invalid("http://127.1.2.3/rule.wasm"); // Entire 127.0.0.0/8 block
    }

    #[test]
    fn test_unspecified_ipv4() {
        assert_invalid("http://0.0.0.0/rule.wasm");
    }

    #[test]
    fn test_private_ipv4() {
        assert_invalid("http://10.0.0.1/rule.wasm");     // 10.0.0.0/8
        assert_invalid("http://172.16.0.1/rule.wasm");    // 172.16.0.0/12
        assert_invalid("http://172.31.255.255/rule.wasm");
        assert_invalid("http://192.168.0.1/rule.wasm");   // 192.168.0.0/16
        assert_invalid("http://192.168.255.255/rule.wasm");
    }

    #[test]
    fn test_link_local_ipv4() {
        assert_invalid("http://169.254.1.1/rule.wasm"); // 169.254.0.0/16
    }

    #[test]
    fn test_loopback_ipv6() {
        assert_invalid("http://[::1]/rule.wasm");
    }

    #[test]
    fn test_unspecified_ipv6() {
        assert_invalid("http://[::]/rule.wasm");
    }

    #[test]
    fn test_unique_local_ipv6() {
        assert_invalid("http://[fc00::1]/rule.wasm"); // fc00::/7 (covers fc00... and fd00...)
        assert_invalid("http://[fd00::1]/rule.wasm");
    }

    #[test]
    fn test_link_local_ipv6() {
        assert_invalid("http://[fe80::1]/rule.wasm");
    }

    #[test]
    fn test_allow_local_bypass() {
        let localhost = Url::parse("http://localhost/rule.wasm").unwrap();
        assert!(validate_url(&localhost, true).is_ok());

        let ip_local = Url::parse("http://127.0.0.1/rule.wasm").unwrap();
        assert!(validate_url(&ip_local, true).is_ok());

        let private_ip = Url::parse("http://192.168.1.1/rule.wasm").unwrap();
        assert!(validate_url(&private_ip, true).is_ok());

        let file_scheme = Url::parse("file:///tmp/test").unwrap();
        // Scheme validation is also bypassed with allow_local=true?
        // Based on implementation: yes, it returns Ok(()) immediately.
        assert!(validate_url(&file_scheme, true).is_ok());
    }

    #[test]
    fn test_ipv4_public_boundaries() {
        // Just outside 10.0.0.0/8
        assert_valid("http://9.255.255.255/rule.wasm");
        assert_valid("http://11.0.0.0/rule.wasm");

        // Just outside 172.16.0.0/12 (172.16.0.0 - 172.31.255.255)
        assert_valid("http://172.15.255.255/rule.wasm");
        assert_valid("http://172.32.0.0/rule.wasm");

        // Just outside 192.168.0.0/16
        assert_valid("http://192.167.255.255/rule.wasm");
        assert_valid("http://192.169.0.0/rule.wasm");
    }
}
