use actix_web::HttpRequest;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;

// https://www.cloudflare.com/ips/
const CLOUDFLARE: &[&str] = &[
    "173.245.48.0/20",
    "103.21.244.0/22",
    "103.22.200.0/22",
    "103.31.4.0/22",
    "141.101.64.0/18",
    "108.162.192.0/18",
    "190.93.240.0/20",
    "188.114.96.0/20",
    "197.234.240.0/22",
    "198.41.128.0/17",
    "162.158.0.0/15",
    "104.16.0.0/13",
    "104.24.0.0/14",
    "172.64.0.0/13",
    "131.0.72.0/22",
    "2400:cb00::/32",
    "2606:4700::/32",
    "2803:f800::/32",
    "2405:b500::/32",
    "2405:8100::/32",
    "2a06:98c0::/29",
    "2c0f:f248::/32",
];

/// Maps IPv4-mapped IPv6 addresses to IPv4.
pub fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        ip => ip,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cidr {
    addr: IpAddr,
    prefix: u8,
}

impl Cidr {
    pub fn new(addr: IpAddr, prefix: u8) -> Option<Cidr> {
        let addr = canonical(addr);
        let max = if addr.is_ipv4() { 32 } else { 128 };
        (prefix <= max).then(|| Cidr {
            addr: mask(addr, prefix),
            prefix,
        })
    }

    /// The range a ban on `ip` covers: the address itself, or its /64 for IPv6.
    pub fn ban_range(ip: IpAddr) -> Cidr {
        let ip = canonical(ip);
        let prefix = if ip.is_ipv4() { 32 } else { 64 };
        Cidr::new(ip, prefix).expect("prefix is in range")
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        let ip = canonical(ip);
        ip.is_ipv4() == self.addr.is_ipv4() && mask(ip, self.prefix) == self.addr
    }
}

fn mask(addr: IpAddr, prefix: u8) -> IpAddr {
    match addr {
        IpAddr::V4(v4) => {
            let bits = u32::from(v4).checked_shr(32 - prefix as u32).unwrap_or(0);
            IpAddr::V4(bits.checked_shl(32 - prefix as u32).unwrap_or(0).into())
        }
        IpAddr::V6(v6) => {
            let bits = u128::from(v6).checked_shr(128 - prefix as u32).unwrap_or(0);
            IpAddr::V6(bits.checked_shl(128 - prefix as u32).unwrap_or(0).into())
        }
    }
}

impl FromStr for Cidr {
    type Err = String;

    fn from_str(s: &str) -> Result<Cidr, String> {
        let (addr, prefix) = match s.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (s, None),
        };
        let addr: IpAddr = addr.trim().parse().map_err(|_| format!("invalid address `{}`", s))?;
        let prefix = match prefix {
            Some(prefix) => prefix.trim().parse().map_err(|_| format!("invalid prefix `{}`", s))?,
            None if canonical(addr).is_ipv4() => 32,
            None => 128,
        };
        Cidr::new(addr, prefix).ok_or_else(|| format!("prefix out of range `{}`", s))
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix)
    }
}

/// An entry of `TRUSTED_PROXIES`.
#[derive(Debug, Clone)]
pub enum TrustedProxy {
    Cidr(Cidr),
    Cloudflare,
}

impl FromStr for TrustedProxy {
    type Err = String;

    fn from_str(s: &str) -> Result<TrustedProxy, String> {
        if s.trim().eq_ignore_ascii_case("cloudflare") {
            Ok(TrustedProxy::Cloudflare)
        } else {
            s.parse().map(TrustedProxy::Cidr)
        }
    }
}

pub struct RealIp {
    trusted: Vec<Cidr>,
    header: String,
}

impl RealIp {
    pub fn new(proxies: &[TrustedProxy], header: String) -> RealIp {
        let trusted = proxies
            .iter()
            .flat_map(|proxy| match proxy {
                TrustedProxy::Cidr(cidr) => vec![*cidr],
                TrustedProxy::Cloudflare => CLOUDFLARE
                    .iter()
                    .map(|range| range.parse().expect("cloudflare ranges are valid"))
                    .collect(),
            })
            .collect();
        RealIp { trusted, header }
    }

    fn is_trusted(&self, ip: IpAddr) -> bool {
        self.trusted.iter().any(|cidr| cidr.contains(ip))
    }

    /// The header is only believed if the peer is a trusted proxy; in a forwarding chain the
    /// rightmost address not belonging to a trusted proxy is the client.
    pub fn resolve(&self, peer: Option<IpAddr>, header: Option<&str>) -> IpAddr {
        let peer = peer.map(canonical).unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        if !self.is_trusted(peer) {
            return peer;
        }
        header
            .into_iter()
            .flat_map(|value| value.rsplit(','))
            .filter_map(|ip| ip.trim().parse::<IpAddr>().ok().map(canonical))
            .find(|ip| !self.is_trusted(*ip))
            .unwrap_or(peer)
    }

    pub fn of(&self, req: &HttpRequest) -> IpAddr {
        let header = req
            .headers()
            .get(self.header.as_str())
            .and_then(|value| value.to_str().ok());
        self.resolve(req.peer_addr().map(|addr| addr.ip()), header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn cidr() {
        let range: Cidr = "10.0.0.0/8".parse().unwrap();
        assert!(range.contains(ip("10.1.2.3")));
        assert!(!range.contains(ip("11.0.0.1")));
        assert!(range.contains(ip("::ffff:10.0.0.1")));
        assert!(!range.contains(ip("::1")));

        assert_eq!("10.1.2.3/8".parse::<Cidr>().unwrap().to_string(), "10.0.0.0/8");
        assert_eq!("10.1.2.3".parse::<Cidr>().unwrap().to_string(), "10.1.2.3/32");
        assert_eq!("0.0.0.0/0".parse::<Cidr>().unwrap().to_string(), "0.0.0.0/0");
        assert!("10.0.0.0/33".parse::<Cidr>().is_err());
        assert!("nonsense".parse::<Cidr>().is_err());

        assert_eq!(Cidr::ban_range(ip("1.2.3.4")).to_string(), "1.2.3.4/32");
        assert_eq!(Cidr::ban_range(ip("2001:db8:1:2:3:4:5:6")).to_string(), "2001:db8:1:2::/64");
        assert!(Cidr::ban_range(ip("2001:db8:1:2::1")).contains(ip("2001:db8:1:2:ffff::")));
    }

    #[test]
    fn resolve() {
        let real_ip = RealIp::new(
            &["127.0.0.0/8".parse().unwrap(), "cloudflare".parse().unwrap()],
            "CF-Connecting-IP".into(),
        );

        // an untrusted peer cannot spoof its address
        assert_eq!(real_ip.resolve(Some(ip("203.0.113.7")), Some("1.1.1.1")), ip("203.0.113.7"));
        // cloudflared on loopback, or a cloudflare edge
        assert_eq!(real_ip.resolve(Some(ip("127.0.0.1")), Some("198.51.100.4")), ip("198.51.100.4"));
        assert_eq!(real_ip.resolve(Some(ip("162.158.1.1")), Some("198.51.100.4")), ip("198.51.100.4"));
        assert_eq!(real_ip.resolve(Some(ip("::ffff:127.0.0.1")), Some("::ffff:198.51.100.4")), ip("198.51.100.4"));
        // forwarding chains and garbage
        assert_eq!(
            real_ip.resolve(Some(ip("127.0.0.1")), Some("6.6.6.6, 198.51.100.4, 162.158.1.1")),
            ip("198.51.100.4")
        );
        assert_eq!(real_ip.resolve(Some(ip("127.0.0.1")), Some("garbage")), ip("127.0.0.1"));
        assert_eq!(real_ip.resolve(Some(ip("127.0.0.1")), None), ip("127.0.0.1"));
    }
}
