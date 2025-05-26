use pnet::ipnetwork::{Ipv4Network, Ipv6Network};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
    sync::LazyLock,
};

/// https://en.wikipedia.org/wiki/Reserved_IP_addresses
pub fn check(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(addr_v4) => check_ipv4(addr_v4),
        IpAddr::V6(addr_v6) => check_ipv6(addr_v6),
    }
}

/// https://en.wikipedia.org/wiki/Reserved_IP_addresses
pub fn check_ipv4(addr: Ipv4Addr) -> bool {
    static RESERVED_RAW: [&str; 17] = [
        "0.0.0.0/8",
        "10.0.0.0/8",
        "100.64.0.0/10",
        "127.0.0.0/8",
        "169.254.0.0/16",
        "172.16.0.0/12",
        "192.0.0.0/24",
        "192.0.2.0/24",
        "192.88.99.0/24",
        "192.168.0.0/16",
        "198.18.0.0/15",
        "198.51.100.0/24",
        "203.0.113.0/24",
        "224.0.0.0/4",
        "233.252.0.0/24",
        "240.0.0.0/4",
        "255.255.255.255/32",
    ];

    static RESERVED: LazyLock<Vec<Ipv4Network>> = LazyLock::new(|| {
        RESERVED_RAW
            .into_iter()
            .map(|x| Ipv4Network::from_str(x).unwrap())
            .collect()
    });

    for reserved_addr in RESERVED.iter() {
        if reserved_addr.contains(addr) {
            return true;
        }
    }

    false
}

/// https://en.wikipedia.org/wiki/Reserved_IP_addresses
pub fn check_ipv6(addr: Ipv6Addr) -> bool {
    static RESERVED_RAW: [&str; 16] = [
        "::/128",
        "::1/128",
        "::ffff:0:0/96",
        "::ffff:0:0:0/96",
        "64:ff9b::/96",
        "64:ff9b:1::/4",
        "100::/64",
        "2001::/32",
        "2001:20::/28",
        "2001:db8::/32",
        "2002::/16",
        "3fff::/20",
        "5f00::/16",
        "fc00::/7",
        "fe80::/10",
        "ff00::/8",
    ];

    static RESERVED: LazyLock<Vec<Ipv6Network>> = LazyLock::new(|| {
        RESERVED_RAW
            .into_iter()
            .map(|x| Ipv6Network::from_str(x).unwrap())
            .collect()
    });

    for reserved_addr in RESERVED.iter() {
        if reserved_addr.contains(addr) {
            return true;
        }
    }

    false
}
