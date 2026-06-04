use std::net::Ipv6Addr;
use std::str::FromStr;

fn ipv4_is_blocked(addr: std::net::Ipv4Addr) -> bool {
    let [a, b, _, _] = addr.octets();
    addr.is_loopback() || addr.is_private() || addr.is_link_local() || addr.is_unspecified() || addr.is_broadcast() || addr.is_documentation() || addr.is_multicast() || a == 0 || (a == 100 && (64..=127).contains(&b)) || a >= 240
}

fn ipv6_is_blocked_old(addr: Ipv6Addr) -> bool {
    if let Some(mapped) = addr.to_ipv4() {
        return ipv4_is_blocked(mapped);
    }
    let segments = addr.segments();
    addr.is_loopback()
        || addr.is_unspecified()
        || addr.is_multicast()
        || (segments[0] & 0xfe00) == 0xfc00
        || (segments[0] & 0xffc0) == 0xfe80
}

fn main() {
    let loopback = Ipv6Addr::from_str("::1").unwrap();
    println!("::1 is blocked old? {}", ipv6_is_blocked_old(loopback));
}
