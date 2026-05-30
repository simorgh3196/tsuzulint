use std::net::{IpAddr, Ipv4Addr};

/// Validates an IP address against the hard-block list, unwrapping transition IPv6 addresses.
/// Rejects loopback, unspecified, link-local (incl. 169.254.169.254), and private addresses
/// unless `allow_private_targets` is true (which only relaxes RFC1918/ULA/CGNAT).
pub fn validate_ip(ip: IpAddr, allow_private_targets: bool) -> Result<(), &'static str> {
    let unwrapped = unwrap_ipv6(ip);

    // Hard-block: unspecified, loopback, link-local (incl metadata)
    if unwrapped.is_unspecified() {
        return Err("unspecified address blocked");
    }
    if unwrapped.is_loopback() {
        return Err("loopback address blocked");
    }

    match unwrapped {
        IpAddr::V4(v4) => {
            if v4.is_link_local() {
                return Err("link-local address blocked");
            }
            if !allow_private_targets && (v4.is_private() || is_cgnat(&v4)) {
                return Err("private address blocked");
            }
        }
        IpAddr::V6(v6) => {
            // Unicast link-local
            if (v6.segments()[0] & 0xffc0) == 0xfe80 {
                return Err("link-local address blocked");
            }
            // ULA (fc00::/7)
            if !allow_private_targets && (v6.segments()[0] & 0xfe00) == 0xfc00 {
                return Err("private address blocked");
            }
        }
    }

    Ok(())
}

fn unwrap_ipv6(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return IpAddr::V4(v4);
            }
            let seg = v6.segments();
            // IPv4-compatible
            if seg[0] == 0
                && seg[1] == 0
                && seg[2] == 0
                && seg[3] == 0
                && seg[4] == 0
                && seg[5] == 0
                && seg[6] == 0
                && seg[7] != 0
                && seg[7] != 1
            {
                return IpAddr::V4(Ipv4Addr::new(
                    (seg[6] >> 8) as u8,
                    seg[6] as u8,
                    (seg[7] >> 8) as u8,
                    seg[7] as u8,
                ));
            }
            // 6to4 (2002::/16)
            if seg[0] == 0x2002 {
                return IpAddr::V4(Ipv4Addr::new(
                    (seg[1] >> 8) as u8,
                    seg[1] as u8,
                    (seg[2] >> 8) as u8,
                    seg[2] as u8,
                ));
            }
            // NAT64 well-known prefix (64:ff9b::/96)
            if seg[0] == 0x0064
                && seg[1] == 0xff9b
                && seg[2] == 0
                && seg[3] == 0
                && seg[4] == 0
                && seg[5] == 0
            {
                return IpAddr::V4(Ipv4Addr::new(
                    (seg[6] >> 8) as u8,
                    seg[6] as u8,
                    (seg[7] >> 8) as u8,
                    seg[7] as u8,
                ));
            }
            ip
        }
        _ => ip,
    }
}

fn is_cgnat(v4: &Ipv4Addr) -> bool {
    let octets = v4.octets();
    octets[0] == 100 && (octets[1] & 0b1100_0000) == 64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    #[test]
    fn test_unwrap_ipv6() {
        let mapped = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x7f00, 0x0001));
        assert_eq!(unwrap_ipv6(mapped), IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));

        let nat64 = IpAddr::V6(Ipv6Addr::new(0x0064, 0xff9b, 0, 0, 0, 0, 0x0a00, 0x0001));
        assert_eq!(unwrap_ipv6(nat64), IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
    }

    #[test]
    fn test_validate_ip() {
        // Loopback is always blocked
        assert!(validate_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), true).is_err());
        assert!(validate_ip(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)), true).is_err());

        // Metadata is always blocked
        assert!(validate_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)), true).is_err());

        // Private is blocked if allow_private_targets=false
        assert!(validate_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), false).is_err());
        assert!(validate_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), true).is_ok());

        // Public is ok
        assert!(validate_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), false).is_ok());
    }
}
