use reqwest::Url;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecurityError {
    #[error("Invalid URL scheme: {0}")]
    InvalidScheme(String),
    #[error("Access to loopback address denied: {0}")]
    LoopbackDenied(String),
    #[error("Access to private IP address denied: {0}")]
    PrivateIpDenied(String),
    #[error("Path traversal detected: {path} escapes {base}")]
    PathTraversal { path: String, base: String },
    #[error("Absolute or rooted path not allowed: {0}")]
    AbsolutePathNotAllowed(String),
    #[error("Parent directory '..' not allowed in path: {0}")]
    ParentDirNotAllowed(String),
    #[error("WASM file not found: {path}")]
    FileNotFound { path: String },
}

/// Checks if an IP address is safe (publicly routable).
pub fn check_ip(ip: std::net::IpAddr) -> Result<(), SecurityError> {
    match ip {
        std::net::IpAddr::V4(ipv4) => {
            if ipv4.is_loopback() || ipv4.is_unspecified() {
                return Err(SecurityError::LoopbackDenied(ipv4.to_string()));
            }
            if ipv4.is_private() || ipv4.is_link_local() {
                return Err(SecurityError::PrivateIpDenied(ipv4.to_string()));
            }
        }
        std::net::IpAddr::V6(ipv6) => {
            // Check for IPv6-mapped IPv4 addresses (e.g., ::ffff:127.0.0.1)
            // These must be checked as IPv4 to prevent SSRF bypass
            if let Some(ipv4) = ipv6.to_ipv4_mapped() {
                return check_ip(std::net::IpAddr::V4(ipv4));
            }
            if ipv6.is_loopback() || ipv6.is_unspecified() {
                return Err(SecurityError::LoopbackDenied(ipv6.to_string()));
            }
            // Unique local (fc00::/7)
            if (ipv6.segments()[0] & 0xfe00) == 0xfc00 || ipv6.is_unicast_link_local() {
                return Err(SecurityError::PrivateIpDenied(ipv6.to_string()));
            }
        }
    }
    Ok(())
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
            check_ip(std::net::IpAddr::V4(ipv4))?;
        }
        Some(url::Host::Ipv6(ipv6)) => {
            check_ip(std::net::IpAddr::V6(ipv6))?;
        }
        None => {}
    }

    Ok(())
}

pub fn validate_local_wasm_path(
    wasm_relative: &Path,
    manifest_dir: &Path,
) -> Result<PathBuf, SecurityError> {
    if wasm_relative.is_absolute() || wasm_relative.has_root() {
        return Err(SecurityError::AbsolutePathNotAllowed(
            wasm_relative.to_string_lossy().to_string(),
        ));
    }

    if wasm_relative
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(SecurityError::ParentDirNotAllowed(
            wasm_relative.to_string_lossy().to_string(),
        ));
    }

    let wasm_path = manifest_dir.join(wasm_relative);

    if !wasm_path.exists() {
        return Err(SecurityError::FileNotFound {
            path: wasm_path.to_string_lossy().to_string(),
        });
    }

    let manifest_canon = manifest_dir
        .canonicalize()
        .map_err(|_| SecurityError::PathTraversal {
            path: wasm_path.to_string_lossy().to_string(),
            base: manifest_dir.to_string_lossy().to_string(),
        })?;

    let wasm_canon = wasm_path
        .canonicalize()
        .map_err(|_| SecurityError::PathTraversal {
            path: wasm_path.to_string_lossy().to_string(),
            base: manifest_dir.to_string_lossy().to_string(),
        })?;

    if !wasm_canon.starts_with(&manifest_canon) {
        return Err(SecurityError::PathTraversal {
            path: wasm_path.display().to_string(),
            base: manifest_dir.display().to_string(),
        });
    }

    Ok(wasm_canon)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_valid(url_str: &str) {
        let url = Url::parse(url_str).unwrap();
        assert!(
            validate_url(&url, false).is_ok(),
            "Expected valid: {}",
            url_str
        );
    }

    fn assert_invalid(url_str: &str) {
        let url = Url::parse(url_str).unwrap();
        assert!(
            validate_url(&url, false).is_err(),
            "Expected invalid: {}",
            url_str
        );
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
        assert_invalid("http://10.0.0.1/rule.wasm"); // 10.0.0.0/8
        assert_invalid("http://172.16.0.1/rule.wasm"); // 172.16.0.0/12
        assert_invalid("http://172.31.255.255/rule.wasm");
        assert_invalid("http://192.168.0.1/rule.wasm"); // 192.168.0.0/16
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

    #[test]
    fn test_check_ip() {
        // Public IP
        let public_ip: std::net::IpAddr = "8.8.8.8".parse().unwrap();
        assert!(check_ip(public_ip).is_ok());

        // Private IP
        let private_ip: std::net::IpAddr = "192.168.1.1".parse().unwrap();
        assert!(check_ip(private_ip).is_err());
    }

    #[test]
    fn test_check_ip_ipv6() {
        // Loopback
        let ip: std::net::IpAddr = "::1".parse().unwrap();
        assert!(
            matches!(check_ip(ip), Err(SecurityError::LoopbackDenied(_))),
            "::1 should be denied as loopback"
        );

        // Unspecified
        let ip: std::net::IpAddr = "::".parse().unwrap();
        assert!(
            matches!(check_ip(ip), Err(SecurityError::LoopbackDenied(_))),
            ":: should be denied as unspecified"
        );

        // Unique local (fd00::/8)
        let ip: std::net::IpAddr = "fd00::1".parse().unwrap();
        assert!(
            matches!(check_ip(ip), Err(SecurityError::PrivateIpDenied(_))),
            "fd00::1 should be denied as unique local"
        );

        // Unique local (fc00::/8)
        let ip: std::net::IpAddr = "fc00::1".parse().unwrap();
        assert!(
            matches!(check_ip(ip), Err(SecurityError::PrivateIpDenied(_))),
            "fc00::1 should be denied as unique local"
        );

        // Link local
        let ip: std::net::IpAddr = "fe80::1".parse().unwrap();
        assert!(
            matches!(check_ip(ip), Err(SecurityError::PrivateIpDenied(_))),
            "fe80::1 should be denied as link local"
        );

        // Public IPv6 (Google DNS)
        let ip: std::net::IpAddr = "2001:4860:4860::8888".parse().unwrap();
        assert!(
            check_ip(ip).is_ok(),
            "2001:4860:4860::8888 should be allowed"
        );
    }

    #[test]
    fn test_check_ip_ipv4_mapped_ipv6() {
        // IPv4-mapped loopback
        let ip: std::net::IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(
            matches!(check_ip(ip), Err(SecurityError::LoopbackDenied(_))),
            "::ffff:127.0.0.1 should be denied as loopback"
        );

        // IPv4-mapped private (10.0.0.0/8)
        let ip: std::net::IpAddr = "::ffff:10.0.0.1".parse().unwrap();
        assert!(
            matches!(check_ip(ip), Err(SecurityError::PrivateIpDenied(_))),
            "::ffff:10.0.0.1 should be denied as private"
        );

        // IPv4-mapped private (172.16.0.0/12)
        let ip: std::net::IpAddr = "::ffff:172.16.0.1".parse().unwrap();
        assert!(
            matches!(check_ip(ip), Err(SecurityError::PrivateIpDenied(_))),
            "::ffff:172.16.0.1 should be denied as private"
        );

        // IPv4-mapped private (192.168.0.0/16)
        let ip: std::net::IpAddr = "::ffff:192.168.1.1".parse().unwrap();
        assert!(
            matches!(check_ip(ip), Err(SecurityError::PrivateIpDenied(_))),
            "::ffff:192.168.1.1 should be denied as private"
        );

        // IPv4-mapped link-local
        let ip: std::net::IpAddr = "::ffff:169.254.1.1".parse().unwrap();
        assert!(
            matches!(check_ip(ip), Err(SecurityError::PrivateIpDenied(_))),
            "::ffff:169.254.1.1 should be denied as link-local"
        );

        // IPv4-mapped unspecified
        let ip: std::net::IpAddr = "::ffff:0.0.0.0".parse().unwrap();
        assert!(
            matches!(check_ip(ip), Err(SecurityError::LoopbackDenied(_))),
            "::ffff:0.0.0.0 should be denied as unspecified"
        );

        // IPv4-mapped public
        let ip: std::net::IpAddr = "::ffff:8.8.8.8".parse().unwrap();
        assert!(
            check_ip(ip).is_ok(),
            "::ffff:8.8.8.8 should be allowed as public"
        );
    }
}
