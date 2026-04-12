use std::net::{IpAddr, Ipv6Addr};

fn check_ip(ip: std::net::IpAddr) -> Result<(), String> {
    match ip {
        std::net::IpAddr::V4(ipv4) => {
            if ipv4.is_loopback() || ipv4.is_unspecified() {
                return Err("LoopbackDenied".to_string());
            }
            if ipv4.is_private() || ipv4.is_link_local() {
                return Err("PrivateIpDenied".to_string());
            }
        }
        std::net::IpAddr::V6(ipv6) => {
            if let Some(ipv4) = ipv6.to_ipv4_mapped() {
                return check_ip(std::net::IpAddr::V4(ipv4));
            }
            if ipv6.is_loopback() || ipv6.is_unspecified() {
                return Err("LoopbackDenied".to_string());
            }
            if (ipv6.segments()[0] & 0xfe00) == 0xfc00 || ipv6.is_unicast_link_local() {
                return Err("PrivateIpDenied".to_string());
            }
        }
    }
    Ok(())
}

fn main() {
    let ip = "::127.0.0.1".parse::<IpAddr>().unwrap();
    println!("checking {}: {:?}", ip, check_ip(ip));

    let ip = "::192.168.1.1".parse::<IpAddr>().unwrap();
    println!("checking {}: {:?}", ip, check_ip(ip));
}
