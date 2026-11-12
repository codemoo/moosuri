use super::BrowserError;
use std::net::{IpAddr, SocketAddr};
use tokio::net::TcpStream;
use url::{Host, Url};

pub(super) fn public_url(raw: &str) -> Result<Url, BrowserError> {
    if raw.len() > 2048 || raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(BrowserError::InvalidUrl);
    }
    let mut url = Url::parse(raw).map_err(|_| BrowserError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host().is_none()
        || raw.split_once("://").is_some_and(|(_, r)| {
            r.split(['/', '?', '#'])
                .next()
                .is_some_and(|a| a.contains('@'))
        })
        || url
            .port()
            .is_some_and(|p| p != if url.scheme() == "http" { 80 } else { 443 })
    {
        return Err(BrowserError::InvalidUrl);
    }
    match url.host().ok_or(BrowserError::InvalidUrl)? {
        Host::Ipv4(ip) if !public_ip(ip.into()) => return Err(BrowserError::ForbiddenDestination),
        Host::Ipv6(ip) if !public_ip(ip.into()) => return Err(BrowserError::ForbiddenDestination),
        Host::Domain(host) if !public_name(host) => return Err(BrowserError::ForbiddenDestination),
        _ => {}
    }
    url.set_fragment(None);
    Ok(url)
}

fn public_name(host: &str) -> bool {
    let h = host.trim_end_matches('.');
    h.contains('.')
        && ![
            "localhost",
            "local",
            "internal",
            "home.arpa",
            "test",
            "invalid",
            "onion",
        ]
        .iter()
        .any(|s| h == *s || h.ends_with(&format!(".{s}")))
}

pub(super) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || ip.octets() == [168, 63, 129, 16]
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 0 || b == 168 || (b == 88 && c == 99)))
                || (a == 198 && ((18..=19).contains(&b) || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_ip(v4.into());
            }
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}

pub(super) async fn public_connect(url: &Url) -> Result<TcpStream, BrowserError> {
    let host = url.host_str().ok_or(BrowserError::InvalidUrl)?;
    let port = url
        .port_or_known_default()
        .ok_or(BrowserError::InvalidUrl)?;
    let addresses: Vec<SocketAddr> = match url.host().ok_or(BrowserError::InvalidUrl)? {
        Host::Ipv4(ip) => vec![SocketAddr::new(ip.into(), port)],
        Host::Ipv6(ip) => vec![SocketAddr::new(ip.into(), port)],
        Host::Domain(_) => tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| BrowserError::Network)?
            .collect(),
    };
    if addresses.is_empty() || addresses.len() > 64 || addresses.iter().any(|a| !public_ip(a.ip()))
    {
        return Err(BrowserError::ForbiddenDestination);
    }
    // No second lookup: each connection uses only these vetted socket addresses.
    for address in addresses {
        if let Ok(Ok(stream)) = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            TcpStream::connect(address),
        )
        .await
        {
            return Ok(stream);
        }
    }
    Err(BrowserError::Network)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cloud_metadata_address_and_mapped_alias_are_never_public() {
        for address in ["168.63.129.16", "::ffff:168.63.129.16"] {
            assert!(!public_ip(address.parse().unwrap()));
        }
        for url in [
            "http://168.63.129.16/",
            "https://168.63.129.16/",
            "http://[::ffff:168.63.129.16]/",
        ] {
            assert_eq!(public_url(url), Err(BrowserError::ForbiddenDestination));
        }
        assert!(public_ip("168.63.129.17".parse().unwrap()));
    }
}
