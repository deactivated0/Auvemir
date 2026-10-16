//! Google Cast v2 control and bounded, token-protected local audio streaming.
use anyhow::{Context, Result, bail, ensure};
use rand::RngCore;
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};
const RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
const CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
const HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const MEDIA: &str = "urn:x-cast:com.google.cast.media";
const LIMIT: usize = 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub name: String,
    pub address: SocketAddr,
}
pub fn discover() -> Result<Vec<Device>> {
    let daemon = mdns_sd::ServiceDaemon::new()?;
    let kind = "_googlecast._tcp.local.";
    let receiver = daemon.browse(kind)?;
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    let mut devices = std::collections::BTreeMap::new();
    while let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) {
        match receiver.recv_timeout(remaining) {
            Ok(mdns_sd::ServiceEvent::ServiceResolved(info)) => {
                for ip in info.get_addresses_v4() {
                    let address = SocketAddr::new(ip.into(), info.get_port());
                    if local(address.ip()) {
                        devices.insert(
                            address,
                            Device {
                                name: info
                                    .get_property_val_str("fn")
                                    .unwrap_or("Cast receiver")
                                    .into(),
                                address,
                            },
                        );
                        break;
                    }
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    daemon.stop_browse(kind).ok();
    daemon.shutdown().ok();
    Ok(devices.into_values().collect())
}
pub(crate) fn local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_link_local(),
        IpAddr::V6(v) => {
            (v.segments()[0] & 0xfe00 == 0xfc00) || (v.segments()[0] & 0xffc0 == 0xfe80)
        }
    }
}
fn varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 128 {
        out.push(value as u8 | 128);
        value >>= 7;
    }
    out.push(value as u8);
}
fn bytes(field: u64, value: &[u8], out: &mut Vec<u8>) {
    varint(field << 3 | 2, out);
    varint(value.len() as u64, out);
    out.extend_from_slice(value);
}
fn read_varint(data: &[u8], at: &mut usize) -> Result<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let b = *data.get(*at).context("Truncated Cast frame")?;
        *at += 1;
        ensure!(shift < 63 || b <= 1, "Invalid Cast varint");
        value |= ((b & 127) as u64) << shift;
        if b < 128 {
            return Ok(value);
        }
    }
    bail!("Invalid Cast varint")
}
#[derive(Debug, PartialEq)]
struct Message {
    source: String,
    destination: String,
    namespace: String,
    payload: String,
}
impl Message {
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.payload.len() + 128);
        out.extend_from_slice(&[8, 0]);
        bytes(2, self.source.as_bytes(), &mut out);
        bytes(3, self.destination.as_bytes(), &mut out);
        bytes(4, self.namespace.as_bytes(), &mut out);
        out.extend_from_slice(&[40, 0]);
        bytes(6, self.payload.as_bytes(), &mut out);
        out
    }
    fn decode(data: &[u8]) -> Result<Self> {
        ensure!(data.len() <= LIMIT, "Cast frame exceeds limit");
        let mut result = Self {
            source: String::new(),
            destination: String::new(),
            namespace: String::new(),
            payload: String::new(),
        };
        let mut at = 0;
        let mut text_payload = true;
        while at < data.len() {
            let key = read_varint(data, &mut at)?;
            ensure!(key >> 3 > 0, "Invalid protobuf field");
            match key & 7 {
                0 => {
                    let value = read_varint(data, &mut at)?;
                    if key >> 3 == 5 {
                        text_payload = value == 0;
                    }
                }
                1 | 5 => {
                    at = at
                        .checked_add(if key & 7 == 1 { 8 } else { 4 })
                        .context("Invalid protobuf length")?;
                    ensure!(at <= data.len(), "Truncated Cast frame");
                }
                2 => {
                    let n = usize::try_from(read_varint(data, &mut at)?)?;
                    let end = at.checked_add(n).context("Invalid protobuf length")?;
                    let data = data.get(at..end).context("Truncated Cast frame")?;
                    match key >> 3 {
                        2 => result.source = std::str::from_utf8(data)?.into(),
                        3 => result.destination = std::str::from_utf8(data)?.into(),
                        4 => result.namespace = std::str::from_utf8(data)?.into(),
                        6 => result.payload = std::str::from_utf8(data)?.into(),
                        _ => {}
                    }
                    at = end;
                }
                _ => bail!("Unsupported protobuf wire type"),
            }
        }
        ensure!(
            text_payload && !result.namespace.is_empty() && !result.payload.is_empty(),
            "Unsupported Cast payload"
        );
        Ok(result)
    }
}
#[derive(Debug)]
struct DeviceCertificate;
impl ServerCertVerifier for DeviceCertificate {
    fn verify_server_cert(
        &self,
        end: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        if end.is_empty() {
            Err(rustls::Error::General("Missing Cast certificate".into()))
        } else {
            Ok(ServerCertVerified::assertion())
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            signature,
            &rustls::crypto::aws_lc_rs::default_provider().signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            signature,
            &rustls::crypto::aws_lc_rs::default_provider().signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::aws_lc_rs::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
type Stream = tokio_rustls::client::TlsStream<TcpStream>;
pub struct Session {
    stream: Stream,
    sender: String,
    transport: String,
    session: String,
    media_session: Option<i64>,
    request: u32,
}
#[derive(Debug, Clone, Default)]
pub struct Status {
    pub position_ms: u64,
    pub playing: bool,
    pub finished: bool,
}
impl Session {
    /// Cast receivers use self-signed certificates. This verifier is confined to the
    /// explicitly selected private address; it never affects HTTP/server clients.
    pub async fn connect(device: &Device) -> Result<Self> {
        ensure!(
            local(device.address.ip()),
            "Choose a Cast receiver on your private network"
        );
        let tcp = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(device.address))
            .await??;
        tcp.set_nodelay(true)?;
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(DeviceCertificate))
        .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let stream = tokio::time::timeout(
            Duration::from_secs(8),
            connector.connect(ServerName::IpAddress(device.address.ip().into()), tcp),
        )
        .await??;
        let mut s = Self {
            stream,
            sender: format!("sender-{:016x}", rand::random::<u64>()),
            transport: String::new(),
            session: String::new(),
            media_session: None,
            request: 0,
        };
        s.send(
            CONNECTION,
            "receiver-0",
            json!({"type":"CONNECT","origin":{}}),
        )
        .await?;
        let response = s
            .request(
                RECEIVER,
                "receiver-0",
                json!({"type":"LAUNCH","appId":"CC1AD845"}),
            )
            .await?;
        let app = response
            .pointer("/status/applications")
            .and_then(Value::as_array)
            .and_then(|apps| apps.iter().find(|a| a["appId"] == "CC1AD845"))
            .context("Receiver did not launch its audio application")?;
        s.transport = app["transportId"]
            .as_str()
            .context("Missing Cast transport")?
            .into();
        s.session = app["sessionId"]
            .as_str()
            .context("Missing Cast session")?
            .into();
        s.send(
            CONNECTION,
            &s.transport.clone(),
            json!({"type":"CONNECT","origin":{}}),
        )
        .await?;
        Ok(s)
    }
    async fn send(&mut self, namespace: &str, destination: &str, payload: Value) -> Result<()> {
        let data = Message {
            source: self.sender.clone(),
            destination: destination.into(),
            namespace: namespace.into(),
            payload: payload.to_string(),
        }
        .encode();
        ensure!(data.len() <= LIMIT, "Cast request exceeds limit");
        tokio::time::timeout(Duration::from_secs(5), async {
            self.stream
                .write_all(&(data.len() as u32).to_be_bytes())
                .await?;
            self.stream.write_all(&data).await?;
            self.stream.flush().await
        })
        .await??;
        Ok(())
    }
    async fn request(
        &mut self,
        namespace: &str,
        destination: &str,
        mut payload: Value,
    ) -> Result<Value> {
        self.request = self.request.wrapping_add(1).max(1);
        let request = self.request;
        payload["requestId"] = request.into();
        self.send(namespace, destination, payload).await?;
        tokio::time::timeout(Duration::from_secs(10), async {
            for _ in 0..128 {
                let n = self.stream.read_u32().await? as usize;
                ensure!(n > 0 && n <= LIMIT, "Cast response exceeds limit");
                let mut data = vec![0; n];
                self.stream.read_exact(&mut data).await?;
                let message = Message::decode(&data)?;
                let value: Value = serde_json::from_str(&message.payload)?;
                if message.namespace == HEARTBEAT && value["type"] == "PING" {
                    self.send(HEARTBEAT, &message.source, json!({"type":"PONG"}))
                        .await?;
                    continue;
                }
                if value["requestId"].as_u64() == Some(request as u64) {
                    ensure!(
                        !matches!(
                            value["type"].as_str(),
                            Some("INVALID_REQUEST" | "LOAD_FAILED" | "LAUNCH_ERROR")
                        ),
                        "Receiver rejected the request"
                    );
                    return Ok(value);
                }
                if message.namespace == CONNECTION && value["type"] == "CLOSE" {
                    bail!("Cast connection closed");
                }
            }
            bail!("Too many unrelated Cast messages")
        })
        .await?
    }
    fn status_from(&mut self, value: Value) -> Status {
        let Some(s) = value["status"].as_array().and_then(|s| s.first()) else {
            return Status::default();
        };
        self.media_session = s["mediaSessionId"].as_i64().or(self.media_session);
        Status {
            position_ms: (s["currentTime"].as_f64().unwrap_or(0.).max(0.) * 1000.)
                .min(u64::MAX as f64) as u64,
            playing: s["playerState"] == "PLAYING",
            finished: s["playerState"] == "IDLE" && s["idleReason"] == "FINISHED",
        }
    }
    pub async fn load(
        &mut self,
        url: &str,
        content_type: &str,
        title: &str,
        artist: &str,
        position: u64,
        live: bool,
    ) -> Result<Status> {
        crate::network::validate_url(url, true)?;
        let response=self.request(MEDIA,&self.transport.clone(),json!({"type":"LOAD","sessionId":self.session,"autoplay":true,"currentTime":position as f64/1000.,"media":{"contentId":url,"contentType":content_type,"streamType":if live{"LIVE"}else{"BUFFERED"},"metadata":{"metadataType":3,"title":title,"artist":artist}}})).await?;
        Ok(self.status_from(response))
    }
    pub async fn status(&mut self) -> Result<Status> {
        let v = self
            .request(MEDIA, &self.transport.clone(), json!({"type":"GET_STATUS"}))
            .await?;
        Ok(self.status_from(v))
    }
    pub async fn control(&mut self, command: &str, value: f64) -> Result<Status> {
        if command == "volume" {
            self.request(
                RECEIVER,
                "receiver-0",
                json!({"type":"SET_VOLUME","volume":{"level":value.clamp(0.,1.)}}),
            )
            .await?;
            return self.status().await;
        }
        let id = self
            .media_session
            .context("No media is loaded on the receiver")?;
        let message = match command {
            "play" => json!({"type":"PLAY","mediaSessionId":id}),
            "pause" => json!({"type":"PAUSE","mediaSessionId":id}),
            "stop" => json!({"type":"STOP","mediaSessionId":id}),
            "seek" => json!({"type":"SEEK","mediaSessionId":id,"currentTime":value.max(0.)/1000.}),
            _ => bail!("Unknown Cast command"),
        };
        let response = self
            .request(MEDIA, &self.transport.clone(), message)
            .await?;
        Ok(self.status_from(response))
    }
}
pub fn mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mp3" | "mp2" => "audio/mpeg",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "m4a" | "m4b" | "alac" => "audio/mp4",
        "aac" => "audio/aac",
        "ogg" | "opus" => "audio/ogg",
        _ => "application/octet-stream",
    }
}
fn range(value: Option<&str>, length: u64) -> Result<(u64, u64, bool)> {
    ensure!(length > 0, "Empty audio file");
    let Some(value) = value else {
        return Ok((0, length - 1, false));
    };
    let value = value.strip_prefix("bytes=").context("Invalid byte range")?;
    ensure!(!value.contains(','), "Multiple ranges are not supported");
    let (start, end) = value.split_once('-').context("Invalid byte range")?;
    let (start, end) = if start.is_empty() {
        let tail: u64 = end.parse()?;
        ensure!(tail > 0, "Invalid suffix range");
        (length.saturating_sub(tail), length - 1)
    } else {
        let start: u64 = start.parse()?;
        let end = if end.is_empty() {
            length - 1
        } else {
            end.parse::<u64>()?.min(length - 1)
        };
        (start, end)
    };
    ensure!(start < length && start <= end, "Byte range is outside file");
    Ok((start, end, true))
}
pub struct LocalFile {
    pub url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for LocalFile {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl LocalFile {
    pub async fn serve(path: PathBuf, receiver: IpAddr) -> Result<Self> {
        ensure!(local(receiver), "Receiver must be on a private network");
        let route = std::net::UdpSocket::bind(if receiver.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })?;
        route.connect(SocketAddr::new(receiver, 8009))?;
        let address = route.local_addr()?.ip();
        ensure!(local(address), "No private route to receiver");
        let listener = TcpListener::bind(SocketAddr::new(address, 0)).await?;
        let endpoint = listener.local_addr()?;
        let mut token = [0u8; 16];
        rand::rng().fill_bytes(&mut token);
        let token: String = token.iter().map(|b| format!("{b:02x}")).collect();
        let uri = format!("/{token}/audio");
        let url = format!("http://{endpoint}{uri}");
        let task = tokio::spawn(async move {
            let slots = Arc::new(Semaphore::new(4));
            let mut clients = tokio::task::JoinSet::new();
            let deadline = tokio::time::sleep(Duration::from_secs(24 * 3600));
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    _=&mut deadline=>break,
                    Some(_)=clients.join_next(),if !clients.is_empty()=>{},
                    accepted=listener.accept()=>{let Ok((stream,_))=accepted else{break};if let Ok(slot)=slots.clone().try_acquire_owned(){let path=path.clone();let uri=uri.clone();clients.spawn(async move{let _slot=slot;let _=tokio::time::timeout(Duration::from_secs(24*3600),serve_client(stream,&path,&uri)).await;});}},
                }
            }
        });
        Ok(Self { url, task })
    }
}
async fn serve_client(mut stream: TcpStream, path: &Path, uri: &str) -> Result<()> {
    let mut header = Vec::with_capacity(2048);
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut buffer = [0u8; 1024];
        while !header.windows(4).any(|s| s == b"\r\n\r\n") {
            let n = stream.read(&mut buffer).await?;
            ensure!(n > 0 && header.len() + n <= 8192, "Invalid HTTP request");
            header.extend_from_slice(&buffer[..n]);
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    let text = std::str::from_utf8(&header)?;
    let mut lines = text.lines();
    let parts: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    if parts.len() != 3 || parts[1] != uri || !matches!(parts[0], "GET" | "HEAD") {
        stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await?;
        return Ok(());
    }
    let length = tokio::fs::metadata(path).await?.len();
    let mut requested = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("range")
        {
            ensure!(requested.is_none(), "Duplicate byte range");
            requested = Some(value.trim());
        }
    }
    let Ok((start, end, partial)) = range(requested, length) else {
        stream.write_all(format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{length}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await?;
        return Ok(());
    };
    let size = end - start + 1;
    let content_range = if partial {
        format!("Content-Range: bytes {start}-{end}/{length}\r\n")
    } else {
        String::new()
    };
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {size}\r\nAccept-Ranges: bytes\r\n{content_range}Access-Control-Allow-Origin: *\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        if partial {
            "206 Partial Content"
        } else {
            "200 OK"
        },
        mime(path)
    );
    stream.write_all(response.as_bytes()).await?;
    if parts[0] == "HEAD" {
        return Ok(());
    }
    let mut file = tokio::fs::File::open(path).await?;
    file.seek(std::io::SeekFrom::Start(start)).await?;
    let mut remaining = size;
    let mut buffer = [0; 65536];
    while remaining > 0 {
        let wanted = (remaining as usize).min(buffer.len());
        let n = file.read(&mut buffer[..wanted]).await?;
        if n == 0 {
            break;
        }
        stream.write_all(&buffer[..n]).await?;
        remaining -= n as u64;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protobuf_round_trip_and_truncation() {
        let message = Message {
            source: "sender".into(),
            destination: "receiver-0".into(),
            namespace: HEARTBEAT.into(),
            payload: "{\"type\":\"PING\"}".into(),
        };
        let encoded = message.encode();
        assert_eq!(Message::decode(&encoded).unwrap(), message);
        assert!(Message::decode(&encoded[..encoded.len() - 1]).is_err());
        assert!(Message::decode(&[0xff; 16]).is_err());
    }
    #[test]
    fn byte_ranges() {
        assert_eq!(range(None, 100).unwrap(), (0, 99, false));
        assert_eq!(range(Some("bytes=10-19"), 100).unwrap(), (10, 19, true));
        assert_eq!(range(Some("bytes=-10"), 100).unwrap(), (90, 99, true));
        assert_eq!(range(Some("bytes=90-"), 100).unwrap(), (90, 99, true));
        for value in [
            "bytes=100-",
            "bytes=20-10",
            "bytes=-0",
            "bytes=0-1,3-4",
            "something",
        ] {
            assert!(range(Some(value), 100).is_err());
        }
    }
    #[tokio::test]
    async fn actual_http_token_range_and_head() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("audio.wav");
        std::fs::write(&path, b"0123456789").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            for _ in 0..3 {
                let (s, _) = listener.accept().await.unwrap();
                serve_client(s, &path, "/secret/audio").await.unwrap();
            }
        });
        let client = reqwest::Client::new();
        let url = format!("http://{address}/secret/audio");
        let r = client
            .get(&url)
            .header("Range", "bytes=2-5")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 206);
        assert_eq!(r.text().await.unwrap(), "2345");
        let r = client.head(&url).send().await.unwrap();
        assert_eq!(r.headers()["content-length"], "10");
        assert!(r.bytes().await.unwrap().is_empty());
        assert_eq!(
            client
                .get(format!("http://{address}/wrong/audio"))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        task.await.unwrap();
    }
}
