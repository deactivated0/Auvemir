//! Opt-in encrypted room control. Peers play their own matching audio; no files or credentials are sent.
use anyhow::{Context, Result, bail, ensure};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
    task::{JoinHandle, JoinSet},
    time::{MissedTickBehavior, timeout},
};
const AAD: &[u8] = b"AUVROOM1";
const LIMIT: usize = 32 * 1024;

pub fn now_ms() -> i64 {
    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    ORIGIN
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Playback {
    pub identity: String,
    pub title: String,
    pub artist: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub playing: bool,
    pub speed: f32,
}
impl Playback {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.identity.len() == 64 && self.identity.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid media identity"
        );
        ensure!(
            self.title.len() <= 8192 && self.artist.len() <= 8192,
            "Room metadata exceeds limits"
        );
        ensure!(
            self.speed.is_finite() && (0.5..=2.).contains(&self.speed),
            "Invalid playback speed"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub revision: u64,
    pub at_ms: i64,
    pub playback: Option<Playback>,
}
impl State {
    fn validate(&self) -> Result<()> {
        ensure!(self.revision > 0 && self.at_ms >= 0, "Invalid room state");
        if let Some(p) = &self.playback {
            p.validate()?;
        }
        Ok(())
    }
    pub fn position_at(&self, host_now: i64) -> u64 {
        let Some(p) = &self.playback else {
            return 0;
        };
        let elapsed = if p.playing {
            host_now.saturating_sub(self.at_ms).max(0) as f64 * p.speed as f64
        } else {
            0.
        };
        let position = p.position_ms.saturating_add(elapsed as u64);
        if p.duration_ms > 0 {
            position.min(p.duration_ms)
        } else {
            position
        }
    }
}
#[derive(Serialize, Deserialize)]
enum Packet {
    Hello {
        version: u8,
    },
    Ping {
        sequence: u64,
        t0: i64,
    },
    Pong {
        sequence: u64,
        t0: i64,
        t1: i64,
        t2: i64,
    },
    State(State),
}
fn encode(packet: &Packet, key: &[u8; 32]) -> Result<Arc<Vec<u8>>> {
    let data = serde_json::to_vec(packet)?;
    ensure!(data.len() <= LIMIT - 40, "Room message exceeds limits");
    let mut nonce = [0; 24];
    rand::rng().fill_bytes(&mut nonce);
    let encrypted = XChaCha20Poly1305::new(key.into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &data,
                aad: AAD,
            },
        )
        .context("Room encryption failed")?;
    let mut frame = Vec::with_capacity(4 + 24 + encrypted.len());
    frame.extend_from_slice(&((24 + encrypted.len()) as u32).to_be_bytes());
    frame.extend_from_slice(&nonce);
    frame.extend_from_slice(&encrypted);
    Ok(Arc::new(frame))
}
fn decode(data: &[u8], key: &[u8; 32]) -> Result<Packet> {
    ensure!(
        (40..=LIMIT).contains(&data.len()),
        "Invalid room message size"
    );
    let decrypted = XChaCha20Poly1305::new(key.into())
        .decrypt(
            XNonce::from_slice(&data[..24]),
            Payload {
                msg: &data[24..],
                aad: AAD,
            },
        )
        .context("Room authentication failed")?;
    Ok(serde_json::from_slice(&decrypted)?)
}
// Read progress lives outside the future so select! cancellation never discards a partial frame.
struct Reader<R> {
    source: R,
    header: [u8; 4],
    header_read: usize,
    body: Vec<u8>,
    body_read: usize,
}
impl<R: AsyncRead + Unpin> Reader<R> {
    fn new(source: R) -> Self {
        Self {
            source,
            header: [0; 4],
            header_read: 0,
            body: Vec::new(),
            body_read: 0,
        }
    }
    async fn packet(&mut self, key: &[u8; 32]) -> Result<Packet> {
        while self.header_read < 4 {
            let n = self
                .source
                .read(&mut self.header[self.header_read..])
                .await?;
            ensure!(n != 0, "Room connection closed");
            self.header_read += n;
        }
        let size = u32::from_be_bytes(self.header) as usize;
        ensure!((40..=LIMIT).contains(&size), "Invalid room message size");
        self.body.resize(size, 0);
        while self.body_read < size {
            let n = self.source.read(&mut self.body[self.body_read..]).await?;
            ensure!(n != 0, "Room connection closed");
            self.body_read += n;
        }
        let packet = decode(&self.body, key)?;
        self.header_read = 0;
        self.body_read = 0;
        Ok(packet)
    }
}
async fn write<W: AsyncWrite + Unpin>(sink: &mut W, frame: &[u8]) -> Result<()> {
    timeout(Duration::from_secs(10), sink.write_all(frame))
        .await
        .context("Room write timed out")??;
    Ok(())
}
fn parse_invitation(input: &str) -> Result<(SocketAddr, [u8; 32])> {
    ensure!(input.len() <= 512, "Invalid room invitation");
    let url = url::Url::parse(input.trim())?;
    ensure!(
        url.scheme() == "auvemir-room"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Invalid room invitation"
    );
    let ip: IpAddr = match url.host().context("Room address is missing")? {
        url::Host::Ipv4(a) => a.into(),
        url::Host::Ipv6(a) => a.into(),
        url::Host::Domain(a) => a
            .parse()
            .context("Use the IP address in the host's invitation")?,
    };
    ensure!(
        !ip.is_unspecified() && !ip.is_multicast(),
        "Invalid room address"
    );
    let port = url.port().context("Room port is missing")?;
    ensure!(port != 0, "Invalid room port");
    let hex = url
        .path()
        .strip_prefix('/')
        .context("Room key is missing")?;
    ensure!(
        hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid room key"
    );
    let mut key = [0; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)?;
    }
    Ok((SocketAddr::new(ip, port), key))
}
struct Publisher {
    revision: u64,
    sender: watch::Sender<Arc<Vec<u8>>>,
}
pub struct Host {
    pub invitation: String,
    publisher: Mutex<Publisher>,
    key: [u8; 32],
    task: JoinHandle<()>,
}
impl Drop for Host {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Host {
    pub async fn start(address: SocketAddr, playback: Option<Playback>) -> Result<Self> {
        ensure!(
            !address.ip().is_unspecified() && !address.ip().is_multicast(),
            "Choose a reachable local IP address"
        );
        if let Some(p) = &playback {
            p.validate()?;
        }
        let listener = TcpListener::bind(address).await?;
        let bound = listener.local_addr()?;
        let mut key = [0; 32];
        rand::rng().fill_bytes(&mut key);
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        let invitation = format!("auvemir-room://{bound}/{hex}");
        let initial = encode(
            &Packet::State(State {
                revision: 1,
                at_ms: now_ms(),
                playback,
            }),
            &key,
        )?;
        let (sender, receiver) = watch::channel(initial);
        let task = tokio::spawn(async move {
            let slots = Arc::new(tokio::sync::Semaphore::new(16));
            let mut peers = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break; };
                        if let Ok(slot) = slots.clone().try_acquire_owned() {
                            let receiver = receiver.clone();
                            peers.spawn(async move { let _slot = slot; let _ = serve_peer(stream, key, receiver).await; });
                        }
                    }
                    _ = peers.join_next(), if !peers.is_empty() => {}
                }
            }
        });
        Ok(Self {
            invitation,
            publisher: Mutex::new(Publisher {
                revision: 1,
                sender,
            }),
            key,
            task,
        })
    }
    pub fn publish(&self, playback: Option<Playback>) -> Result<()> {
        ensure!(!self.task.is_finished(), "Room listener stopped");
        if let Some(p) = &playback {
            p.validate()?;
        }
        let mut publisher = self
            .publisher
            .lock()
            .map_err(|_| anyhow::anyhow!("Room state lock failed"))?;
        let revision = publisher
            .revision
            .checked_add(1)
            .context("Room revision exhausted")?;
        let frame = encode(
            &Packet::State(State {
                revision,
                at_ms: now_ms(),
                playback,
            }),
            &self.key,
        )?;
        publisher.revision = revision;
        publisher.sender.send_replace(frame);
        Ok(())
    }
}
async fn serve_peer(
    stream: TcpStream,
    key: [u8; 32],
    mut updates: watch::Receiver<Arc<Vec<u8>>>,
) -> Result<()> {
    stream.set_nodelay(true)?;
    let (source, mut sink) = stream.into_split();
    let mut reader = Reader::new(source);
    ensure!(
        matches!(
            timeout(Duration::from_secs(5), reader.packet(&key)).await??,
            Packet::Hello { version: 1 }
        ),
        "Unsupported room handshake"
    );
    let initial = updates.borrow_and_update().clone();
    write(&mut sink, &initial).await?;
    let mut received = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(received + Duration::from_secs(30)) => bail!("Room peer timed out"),
            changed = updates.changed() => { changed?; let frame = updates.borrow_and_update().clone(); write(&mut sink, &frame).await?; }
            packet = reader.packet(&key) => {
                received = tokio::time::Instant::now();
                match packet? {
                    Packet::Ping { sequence, t0 } => {
                        let t1 = now_ms(); let frame = encode(&Packet::Pong { sequence, t0, t1, t2: now_ms() }, &key)?; write(&mut sink, &frame).await?;
                    }
                    _ => bail!("Unexpected room message"),
                }
            }
        }
    }
}
#[derive(Debug)]
pub enum Event {
    State(State, i64),
    Disconnected(String),
}
pub struct Guest {
    task: JoinHandle<()>,
}
impl Drop for Guest {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Guest {
    pub async fn connect(
        input: &str,
        notify: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Self> {
        let (address, key) = parse_invitation(input)?;
        let stream = timeout(Duration::from_secs(10), TcpStream::connect(address))
            .await
            .context("Room connection timed out")??;
        stream.set_nodelay(true)?;
        let (source, mut sink) = stream.into_split();
        let mut reader = Reader::new(source);
        write(&mut sink, &encode(&Packet::Hello { version: 1 }, &key)?).await?;
        let Packet::State(mut state) =
            timeout(Duration::from_secs(10), reader.packet(&key)).await??
        else {
            bail!("Invalid room handshake");
        };
        state.validate()?;
        let task = tokio::spawn(async move {
            let result: Result<()> = async {
                let mut delivered = 0; let mut clock = None; let mut best_rtt = i64::MAX;
                let mut calibrated = Instant::now(); let mut sequence = 0u64; let mut outstanding = None;
                let mut received = tokio::time::Instant::now(); let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
                heartbeat.set_missed_tick_behavior(MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep_until(received + Duration::from_secs(30)) => bail!("Room host timed out"),
                        _ = heartbeat.tick() => {
                            sequence = sequence.wrapping_add(1); let t0 = now_ms(); outstanding = Some((sequence,t0));
                            write(&mut sink, &encode(&Packet::Ping { sequence, t0 }, &key)?).await?;
                        }
                        packet = reader.packet(&key) => {
                            received = tokio::time::Instant::now();
                            match packet? {
                                Packet::State(next) => { next.validate()?; if next.revision > state.revision { state = next; } }
                                Packet::Pong { sequence, t0, t1, t2 } => {
                                    ensure!(outstanding.take() == Some((sequence,t0)) && t1 >= 0 && t2 >= t1, "Invalid room clock response");
                                    let t3 = now_ms(); let rtt = (t3 as i128 - t0 as i128) - (t2 as i128 - t1 as i128);
                                    let offset = ((t1 as i128 - t0 as i128) + (t2 as i128 - t3 as i128)) / 2;
                                    if (0..=i64::MAX as i128).contains(&rtt) && (clock.is_none() || rtt < best_rtt as i128 || calibrated.elapsed() >= Duration::from_secs(60)) {
                                        clock = Some(i64::try_from(offset).context("Invalid room clock offset")?); best_rtt = rtt as i64; calibrated = Instant::now();
                                    }
                                }
                                _ => bail!("Unexpected room message"),
                            }
                        }
                    }
                    if let Some(offset) = clock && state.revision > delivered { delivered = state.revision; notify(Event::State(state.clone(), offset)); }
                }
            }.await;
            if let Err(error) = result {
                notify(Event::Disconnected(error.to_string()));
            }
        });
        Ok(Self { task })
    }
}
impl crate::database::Store {
    pub fn shared_media(&self, identity: &str) -> Result<Option<crate::model::Media>> {
        let mut q = self.conn.prepare_cached("SELECT id,kind,title,artist,album,location,duration_ms,artwork,favorite,provider,group_id,track_number FROM media ORDER BY id")?;
        for row in q.query_map([], crate::database::decode_media)? {
            let media = row?;
            if crate::sync::identity(&media) == identity
                && (media.is_remote()
                    || std::path::Path::new(&media.location).is_file()
                    || self
                        .download_path(&media.id)?
                        .is_some_and(|path| std::path::Path::new(&path).is_file()))
            {
                return Ok(Some(media));
            }
        }
        Ok(None)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matching_audio_skips_missing_duplicates_and_accepts_restored_files() {
        use crate::{
            database::Store,
            model::{Media, MediaKind},
        };
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path().join("library.sqlite")).unwrap();
        let absent = directory.path().join("absent.wav");
        let available = directory.path().join("available.wav");
        std::fs::write(&available, b"audio fixture").unwrap();
        let mut first = Media::new(
            MediaKind::LocalMusic,
            "local",
            absent.to_str().unwrap(),
            "Same track",
        );
        first.id = "a-missing".into();
        let mut second = first.clone();
        second.id = "b-available".into();
        second.location = available.to_string_lossy().into_owned();
        let identity = crate::sync::identity(&first);
        store.upsert(&[first.clone(), second.clone()]).unwrap();
        assert_eq!(store.shared_media(&identity).unwrap(), Some(second));
        std::fs::remove_file(&available).unwrap();
        assert!(store.shared_media(&identity).unwrap().is_none());
        std::fs::write(&absent, b"restored fixture").unwrap();
        assert_eq!(store.shared_media(&identity).unwrap(), Some(first.clone()));
        std::fs::remove_file(&absent).unwrap();
        let downloaded = directory.path().join("downloaded.wav");
        std::fs::write(&downloaded, b"download fixture").unwrap();
        store
            .attach_download(&first.id, downloaded.to_str().unwrap())
            .unwrap();
        assert_eq!(store.shared_media(&identity).unwrap(), Some(first));
    }
    fn playback() -> Playback {
        Playback {
            identity: "a".repeat(64),
            title: "Test".into(),
            artist: "Artist".into(),
            position_ms: 1000,
            duration_ms: 10000,
            playing: true,
            speed: 1.5,
        }
    }
    #[test]
    fn encrypted_state_and_speed_clamping() {
        let state = State {
            revision: 1,
            at_ms: 100,
            playback: Some(playback()),
        };
        let key = [7; 32];
        let frame = encode(&Packet::State(state.clone()), &key).unwrap();
        assert!(!frame.windows(4).any(|s| s == b"Test"));
        assert!(decode(&frame[4..], &[8; 32]).is_err());
        assert!(matches!(decode(&frame[4..], &key).unwrap(), Packet::State(s) if s == state));
        let mut tampered = frame[4..].to_vec();
        tampered[30] ^= 1;
        assert!(decode(&tampered, &key).is_err());
        assert_eq!(state.position_at(1100), 2500);
        assert_eq!(state.position_at(0), 1000);
        assert_eq!(state.position_at(i64::MAX), 10000);
    }
    #[tokio::test]
    async fn cancelled_partial_reads_keep_frame_progress() {
        let key = [3; 32];
        let frame = encode(&Packet::Hello { version: 1 }, &key).unwrap();
        let (mut sink, source) = tokio::io::duplex(1024);
        let mut reader = Reader::new(source);
        sink.write_all(&frame[..2]).await.unwrap();
        assert!(
            timeout(Duration::from_millis(10), reader.packet(&key))
                .await
                .is_err()
        );
        sink.write_all(&frame[2..9]).await.unwrap();
        assert!(
            timeout(Duration::from_millis(10), reader.packet(&key))
                .await
                .is_err()
        );
        sink.write_all(&frame[9..]).await.unwrap();
        assert!(matches!(
            reader.packet(&key).await.unwrap(),
            Packet::Hello { version: 1 }
        ));
    }
    #[tokio::test]
    async fn real_peer_clock_state_and_disconnect() {
        let host = Host::start("127.0.0.1:0".parse().unwrap(), Some(playback()))
            .await
            .unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let _guest = Guest::connect(&host.invitation, move |event| {
            let _ = tx.send(event);
        })
        .await
        .unwrap();
        let first = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(first, Event::State(State { revision: 1, .. }, _)));
        let mut next = playback();
        next.playing = false;
        host.publish(Some(next)).unwrap();
        let second = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(second, Event::State(State { revision: 2, .. }, _)));
        drop(host);
        assert!(matches!(
            timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap()
                .unwrap(),
            Event::Disconnected(_)
        ));
    }
    #[test]
    fn malformed_invitations_are_rejected() {
        for value in [
            "https://example.com",
            "auvemir-room://0.0.0.0:123/a",
            "auvemir-room://example.com:123/a",
            "auvemir-room://127.0.0.1:0/a",
        ] {
            assert!(parse_invitation(value).is_err());
        }
        assert!(
            parse_invitation(&format!(
                "auvemir-room://127.0.0.1:123/{}?x=y",
                "a".repeat(64)
            ))
            .is_err()
        );
    }
}
