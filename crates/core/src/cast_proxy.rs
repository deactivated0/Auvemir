//! Token-scoped relay for a single authorized server item. No arbitrary target URLs.
use anyhow::{Context, Result, ensure};
use rand::RngCore;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};

pub struct RemoteAudio {
    pub url: String,
    pub content_type: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for RemoteAudio {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl RemoteAudio {
    pub async fn serve(
        location: String,
        client: reqwest::Client,
        receiver: IpAddr,
    ) -> Result<Self> {
        ensure!(
            crate::cast::local(receiver),
            "Receiver must be on a private network"
        );
        let upstream = crate::network::validate_url(&location, true)?;
        let route = std::net::UdpSocket::bind(if receiver.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })?;
        route.connect(SocketAddr::new(receiver, 8009))?;
        let address = route.local_addr()?.ip();
        ensure!(crate::cast::local(address), "No private route to receiver");
        let listener = TcpListener::bind(SocketAddr::new(address, 0)).await?;
        let mut token = [0u8; 16];
        rand::rng().fill_bytes(&mut token);
        let token = token.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let uri = format!("/{token}/audio");
        let url = format!("http://{}{uri}", listener.local_addr()?);
        let fallback = crate::cast::mime(std::path::Path::new(upstream.path())).to_owned();
        let content_type =
            tokio::time::timeout(Duration::from_secs(8), client.head(upstream.clone()).send())
                .await
                .ok()
                .and_then(Result::ok)
                .filter(|r| r.status().is_success())
                .and_then(|r| {
                    r.headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned)
                })
                .filter(|s| s.len() <= 256)
                .unwrap_or(fallback);
        let task = tokio::spawn(async move {
            let slots = Arc::new(Semaphore::new(4));
            let mut clients = tokio::task::JoinSet::new();
            let deadline = tokio::time::sleep(Duration::from_secs(24 * 3600));
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    _=&mut deadline=>break,
                    Some(_)=clients.join_next(),if !clients.is_empty()=>{},
                    accepted=listener.accept()=>{
                        let Ok((stream,peer))=accepted else{break};
                        if peer.ip()!=receiver {continue;}
                        if let Ok(slot)=slots.clone().try_acquire_owned(){
                            let client=client.clone();let upstream=upstream.clone();let uri=uri.clone();
                            clients.spawn(async move{let _slot=slot;let _=tokio::time::timeout(Duration::from_secs(24*3600),proxy_connection(stream,&uri,&upstream,&client)).await;});
                        }
                    }
                }
            }
        });
        Ok(Self {
            url,
            content_type,
            task,
        })
    }
}
async fn proxy_connection(
    mut stream: TcpStream,
    uri: &str,
    upstream: &url::Url,
    client: &reqwest::Client,
) -> Result<()> {
    let mut bytes = Vec::with_capacity(2048);
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut block = [0u8; 1024];
        while !bytes.windows(4).any(|b| b == b"\r\n\r\n") {
            let n = stream.read(&mut block).await?;
            ensure!(n > 0 && bytes.len() + n <= 8192, "Invalid relay request");
            bytes.extend_from_slice(&block[..n]);
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let text = std::str::from_utf8(&bytes)?;
    let mut lines = text.lines();
    let mut parts = lines.next().unwrap_or("").split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    let version = parts.next().unwrap_or("");
    if path != uri
        || !matches!(method, "GET" | "HEAD")
        || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
        || parts.next().is_some()
    {
        stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await?;
        return Ok(());
    }
    let mut range = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("range")
        {
            let value = value.trim();
            ensure!(
                range.is_none()
                    && value.len() <= 128
                    && value.starts_with("bytes=")
                    && !value.contains(','),
                "Invalid relay range"
            );
            ensure!(
                value[6..].bytes().all(|b| b.is_ascii_digit() || b == b'-')
                    && value[6..].bytes().filter(|b| *b == b'-').count() == 1,
                "Invalid relay range"
            );
            range = Some(value);
        }
    }
    let mut request = client.request(
        if method == "HEAD" {
            reqwest::Method::HEAD
        } else {
            reqwest::Method::GET
        },
        upstream.clone(),
    );
    if let Some(value) = range {
        request = request.header(reqwest::header::RANGE, value);
    }
    let response = tokio::time::timeout(Duration::from_secs(30), request.send()).await;
    let Ok(Ok(mut response)) = response else {
        stream
            .write_all(
                b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await?;
        return Ok(());
    };
    let status = response.status();
    if !matches!(status.as_u16(), 200 | 206 | 416) {
        stream
            .write_all(
                b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await?;
        return Ok(());
    }
    let mut header = format!(
        "HTTP/1.1 {} {}\r\nConnection: close\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("Response")
    );
    for name in [
        reqwest::header::CONTENT_TYPE,
        reqwest::header::CONTENT_LENGTH,
        reqwest::header::CONTENT_RANGE,
        reqwest::header::ACCEPT_RANGES,
    ] {
        if let Some(value) = response.headers().get(&name) {
            header.push_str(name.as_str());
            header.push_str(": ");
            header.push_str(value.to_str().context("Invalid upstream header")?);
            header.push_str("\r\n");
        }
    }
    header.push_str("\r\n");
    stream.write_all(header.as_bytes()).await?;
    if method != "HEAD" {
        while let Some(chunk) = response.chunk().await? {
            stream.write_all(&chunk).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn relay_forwards_ranges_without_forwarding_receiver_headers_or_upstream_secrets() {
        let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream = url::Url::parse(&format!(
            "http://{}/private?token=secret",
            origin.local_addr().unwrap()
        ))
        .unwrap();
        let origin_task = tokio::spawn(async move {
            let (mut socket, _) = origin.accept().await.unwrap();
            let mut b = [0; 4096];
            let n = socket.read(&mut b).await.unwrap();
            socket.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Type: audio/mpeg\r\nContent-Range: bytes 2-4/10\r\nContent-Length: 3\r\nSet-Cookie: private=secret\r\nConnection: close\r\n\r\ncde").await.unwrap();
            String::from_utf8(b[..n].to_vec()).unwrap()
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            "Bearer owner-secret".parse().unwrap(),
        );
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            proxy_connection(stream, "/random/audio", &upstream, &client)
                .await
                .unwrap()
        });
        let mut receiver = TcpStream::connect(endpoint).await.unwrap();
        receiver.write_all(b"GET /random/audio HTTP/1.1\r\nHost: relay\r\nRange: bytes=2-4\r\nAuthorization: Bearer receiver-secret\r\n\r\n").await.unwrap();
        let mut out = String::new();
        receiver.read_to_string(&mut out).await.unwrap();
        task.await.unwrap();
        let request = origin_task.await.unwrap();
        assert!(request.contains("range: bytes=2-4"));
        assert!(request.contains("Bearer owner-secret"));
        assert!(!request.contains("receiver-secret"));
        assert!(out.starts_with("HTTP/1.1 206"));
        assert!(out.ends_with("cde"));
        assert!(!out.contains("secret"));
        assert!(!out.to_ascii_lowercase().contains("set-cookie"));
    }
    #[tokio::test]
    async fn unknown_token_never_opens_the_upstream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            proxy_connection(
                stream,
                "/right/audio",
                &url::Url::parse("http://127.0.0.1:1/private").unwrap(),
                &reqwest::Client::new(),
            )
            .await
            .unwrap()
        });
        let mut receiver = TcpStream::connect(address).await.unwrap();
        receiver
            .write_all(b"GET /wrong/audio HTTP/1.1\r\nHost: relay\r\n\r\n")
            .await
            .unwrap();
        let mut out = String::new();
        receiver.read_to_string(&mut out).await.unwrap();
        assert!(out.starts_with("HTTP/1.1 404"));
        task.await.unwrap();
    }
}
