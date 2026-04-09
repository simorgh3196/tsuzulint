#[test]
fn test_ssrf_bypass() {
    let ipv4_compatible: std::net::IpAddr = "::127.0.0.1".parse().unwrap();
    let result = tsuzulint_registry::security::check_ip(ipv4_compatible);
    assert!(result.is_err(), "Vulnerable to SSRF bypass!");
}
