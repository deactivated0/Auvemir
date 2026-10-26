use anyhow::{Result, bail};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    sync::{Arc, OnceLock},
    time::Duration,
};
use url::{Host, Url};

pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_multicast()
                || v.is_broadcast()
                || v.octets()[0] == 0
                || v.octets()[0] >= 240
                || (v.octets()[0] == 100 && (64..=127).contains(&v.octets()[1])))
        }
        IpAddr::V6(v) => v
            .to_ipv4_mapped()
            .map(|v| public_ip(IpAddr::V4(v)))
            .unwrap_or_else(|| {
                !(v.is_loopback()
                    || v.is_unspecified()
                    || v.is_multicast()
                    || (v.segments()[0] & 0xfe00 == 0xfc00)
                    || (v.segments()[0] & 0xffc0 == 0xfe80))
            }),
    }
}
pub fn validate_url(input: &str, allow_local: bool) -> Result<Url> {
    let u = Url::parse(input.trim())?;
    if !matches!(u.scheme(), "http" | "https") {
        bail!("Use an http:// or https:// URL");
    }
    if !u.username().is_empty() || u.password().is_some() {
        bail!("Credentials must not be embedded in URLs");
    }
    match u.host() {
        None => bail!("URL needs a hostname"),
        Some(Host::Ipv4(v)) if !allow_local && !public_ip(IpAddr::V4(v)) => {
            bail!("Private network address is disabled")
        }
        Some(Host::Ipv6(v)) if !allow_local && !public_ip(IpAddr::V6(v)) => {
            bail!("Private network address is disabled")
        }
        Some(Host::Domain(s))
            if !allow_local
                && (s == "localhost"
                    || s.ends_with(".localhost")
                    || s.ends_with(".local")
                    || !s.contains('.')) =>
        {
            bail!("Private hostname is disabled")
        }
        _ => {}
    }
    Ok(u)
}
#[derive(Debug)]
struct PublicDns;
impl Resolve for PublicDns {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addresses = tokio::task::spawn_blocking(move || {
                (name.as_str(), 0)
                    .to_socket_addrs()
                    .map(|a| a.collect::<Vec<SocketAddr>>())
            })
            .await??;
            if addresses.is_empty() || addresses.iter().any(|a| !public_ip(a.ip())) {
                return Err("Hostname resolves to a private or invalid address".into());
            }
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}
pub fn client(allow_local: bool) -> Result<reqwest::Client> {
    if allow_local {
        return build_client(true);
    }

    // Public catalog, metadata, artwork, and media requests share one connection
    // pool. Local server clients keep their separate redirect and DNS policy.
    static PUBLIC_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = PUBLIC_CLIENT.get() {
        return Ok(client.clone());
    }

    let client = build_client(false)?;
    let _ = PUBLIC_CLIENT.set(client.clone());
    Ok(PUBLIC_CLIENT.get().cloned().unwrap_or(client))
}

fn build_client(allow_local: bool) -> Result<reqwest::Client> {
    let mut b = reqwest::Client::builder()
        .user_agent("Auvemir/0.1 (open-source audio player)")
        .connect_timeout(Duration::from_secs(12))
        .read_timeout(Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("Too many redirects")
            } else if let Err(e) = validate_url(attempt.url().as_str(), allow_local) {
                attempt.error(e.to_string())
            } else {
                attempt.follow()
            }
        }));
    if !allow_local {
        b = b.dns_resolver(Arc::new(PublicDns));
    }
    Ok(b.build()?)
}
pub async fn text_limited(client: &reqwest::Client, url: &str, max: usize) -> Result<Vec<u8>> {
    validate_url(url, false)?;
    let mut r = client.get(url).send().await?.error_for_status()?;
    if r.content_length().is_some_and(|n| n > max as u64) {
        bail!("Response is too large");
    }
    let mut data = Vec::new();
    while let Some(c) = r.chunk().await? {
        if data.len() + c.len() > max {
            bail!("Response is too large");
        }
        data.extend_from_slice(&c);
    }
    Ok(data)
}
