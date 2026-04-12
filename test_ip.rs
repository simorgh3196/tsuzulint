use std::net::IpAddr;

fn main() {
    let ipv6: std::net::Ipv6Addr = "::1".parse().unwrap();
    println!("ipv6: {}", ipv6);
    if let Some(ipv4) = ipv6.to_ipv4() {
        println!("ipv4: {}", ipv4);
        println!("loopback: {}", ipv4.is_loopback());
        println!("unspecified: {}", ipv4.is_unspecified());
        println!("private: {}", ipv4.is_private());
    } else {
        println!("not an ipv4 compatible or mapped address");
    }
}
