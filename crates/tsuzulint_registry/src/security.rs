use std::net::IpAddr;
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

    if let Some(host_str) = url.host_str() {
        if host_str == "localhost" {
            return Err(SecurityError::LoopbackDenied(host_str.to_string()));
        }

        if let Ok(ip) = host_str.parse::<IpAddr>() {
            if ip.is_loopback() || ip.is_unspecified() {
                return Err(SecurityError::LoopbackDenied(ip.to_string()));
            }

            match ip {
                IpAddr::V4(ipv4) => {
                    if ipv4.is_private() || ipv4.is_link_local() {
                        return Err(SecurityError::PrivateIpDenied(ipv4.to_string()));
                    }
                }
                IpAddr::V6(ipv6) => {
                    // Unique local (fc00::/7)
                    if (ipv6.segments()[0] & 0xfe00) == 0xfc00 || ipv6.is_unicast_link_local() {
                        return Err(SecurityError::PrivateIpDenied(ipv6.to_string()));
                    }
                }
            }
        }
    }

    Ok(())
}
