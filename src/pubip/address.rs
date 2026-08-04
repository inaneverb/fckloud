use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Reports whether the rest of the Internet could route to this address.
///
/// `IpAddr::is_global` answers exactly this, but it is still unstable, so the
/// ranges std keeps behind that feature gate are spelled out here instead.
/// <https://en.wikipedia.org/wiki/Reserved_IP_addresses>
pub fn is_public(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(addr) => is_public_v4(*addr),
        IpAddr::V6(addr) => is_public_v6(addr),
    }
}

fn is_public_v4(addr: Ipv4Addr) -> bool {
    let [a, b, c, _] = addr.octets();

    let shared = a == 100 && (b & 0b1100_0000) == 0b0100_0000; // 100.64.0.0/10
    let benchmarking = a == 198 && (b & 0xfe) == 18; // 198.18.0.0/15
    let reserved = a >= 240; // 240.0.0.0/4
    let protocol = a == 192 && b == 0 && c == 0; // 192.0.0.0/24
    let relay_6to4 = a == 192 && b == 88 && c == 99; // 192.88.99.0/24

    !(addr.is_unspecified()
        || addr.is_loopback()
        || addr.is_private()
        || addr.is_link_local()
        || addr.is_documentation()
        || addr.is_multicast()
        || addr.is_broadcast()
        || shared
        || benchmarking
        || reserved
        || protocol
        || relay_6to4)
}

fn is_public_v6(addr: &Ipv6Addr) -> bool {
    let [a, b, ..] = addr.segments();

    let unique_local = (a & 0xfe00) == 0xfc00; // fc00::/7
    let link_local = (a & 0xffc0) == 0xfe80; // fe80::/10
    let documentation = a == 0x2001 && b == 0x0db8; // 2001:db8::/32

    !(addr.is_unspecified()
        || addr.is_loopback()
        || addr.is_multicast()
        || unique_local
        || link_local
        || documentation)
}

#[cfg(test)]
mod tests {
    use {super::*, std::str::FromStr};

    fn public(s: &str) -> bool {
        is_public(&IpAddr::from_str(s).expect("test address must parse"))
    }

    #[test]
    fn routable_addresses_are_public() {
        assert!(public("1.1.1.1"));
        assert!(public("8.8.8.8"));
        assert!(public("2606:4700:4700::1111"));
    }

    #[test]
    fn reserved_v4_ranges_are_not_public() {
        for addr in [
            "0.0.0.0",
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.0.1",
            "100.64.0.1",
            "198.18.0.1",
            "192.0.2.1",
            "192.0.0.1",
            "192.88.99.1",
            "240.0.0.1",
            "224.0.0.1",
            "239.255.255.255",
            "255.255.255.255",
        ] {
            assert!(!public(addr), "{addr} must not count as public");
        }
    }

    #[test]
    fn reserved_v6_ranges_are_not_public() {
        for addr in [
            "::",
            "::1",
            "fc00::1",
            "fd00::1",
            "fe80::1",
            "2001:db8::1",
            "ff02::1",
        ] {
            assert!(!public(addr), "{addr} must not count as public");
        }
    }

    #[test]
    fn neighbours_of_reserved_ranges_stay_public() {
        assert!(public("100.63.255.255"));
        assert!(public("100.128.0.1"));
        assert!(public("198.17.255.255"));
        assert!(public("198.20.0.1"));
        assert!(public("2001:0db9::1"));
        assert!(public("fbff::1"));
    }
}
