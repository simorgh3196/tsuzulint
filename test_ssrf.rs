use std::net::IpAddr;

fn main() {
    let ipv6: std::net::Ipv6Addr = "::1".parse().unwrap();
    println!("ipv6.is_loopback(): {}", ipv6.is_loopback());
    let ipv4 = ipv6.to_ipv4().unwrap();
    println!("to_ipv4: {}", ipv4);
    println!("ipv4.is_loopback(): {}", ipv4.is_loopback());
    println!("ipv4.is_unspecified(): {}", ipv4.is_unspecified());
    println!("ipv4.is_private(): {}", ipv4.is_private());

    let ip2: std::net::Ipv6Addr = "::127.0.0.1".parse().unwrap();
    println!("ip2 to_ipv4: {:?}", ip2.to_ipv4());
}
