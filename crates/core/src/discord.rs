//! Optional local Discord IPC. No Discord token or account credentials are used.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::watch,
};
#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub started: i64,
    pub ends: Option<i64>,
}
trait Ipc: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Ipc for T {}
type Stream = Box<dyn Ipc>;
struct AbortRead(tokio::task::JoinHandle<()>);
impl Drop for AbortRead {
    fn drop(&mut self) {
        self.0.abort();
    }
}
pub struct Presence {
    tx: watch::Sender<Option<Activity>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Presence {
    fn drop(&mut self) {
        self.tx.send_replace(None);
        self.task.abort();
    }
}
impl Presence {
    pub fn new(runtime: &tokio::runtime::Runtime, app_id: String) -> Result<Self> {
        ensure!(
            (17..=21).contains(&app_id.len()) && app_id.bytes().all(|b| b.is_ascii_digit()),
            "Enter a numeric Discord application ID"
        );
        let (tx, mut rx) = watch::channel(None);
        let task=runtime.spawn(async move{
            loop {
                while rx.borrow().is_none(){if rx.changed().await.is_err(){return;}}
                if let Err(e)=session(&app_id,&mut rx).await{tracing::debug!("Discord presence unavailable: {e}");}
                tokio::select!{_ = tokio::time::sleep(Duration::from_secs(30))=>{},changed=rx.changed()=>{if changed.is_err(){return;}}}
            }
        });
        Ok(Self { tx, task })
    }
    pub fn update(&self, mut activity: Option<Activity>) {
        self.tx.send_if_modified(|old| {
            if let (Some(previous), Some(next)) = (old.as_ref(), activity.as_mut())
                && previous.title == next.title
                && previous.artist == next.artist
                && previous.album == next.album
                && (previous.started - next.started).abs() <= 2
            {
                next.started = previous.started;
                next.ends = previous.ends;
            }
            if *old == activity {
                false
            } else {
                *old = activity;
                true
            }
        });
    }
}
async fn connect() -> Result<Stream> {
    for i in 0..10 {
        #[cfg(unix)]
        {
            let mut roots = Vec::new();
            for key in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
                if let Some(path) = std::env::var_os(key) {
                    roots.push(std::path::PathBuf::from(path));
                }
            }
            roots.push(std::path::PathBuf::from("/tmp"));
            for root in roots {
                for extra in ["", "app/com.discordapp.Discord", "app/dev.vencord.Vesktop"] {
                    let path = root.join(extra).join(format!("discord-ipc-{i}"));
                    if let Ok(socket) = tokio::net::UnixStream::connect(path).await {
                        return Ok(Box::new(socket));
                    }
                }
            }
        }
        #[cfg(windows)]
        {
            if let Ok(pipe) = tokio::net::windows::named_pipe::ClientOptions::new()
                .open(format!("\\\\?\\pipe\\discord-ipc-{i}"))
            {
                return Ok(Box::new(pipe));
            }
        }
    }
    bail!("Discord is not running or its IPC socket is unavailable")
}
async fn send(
    writer: &mut (impl AsyncWrite + Unpin + ?Sized),
    opcode: u32,
    value: &Value,
) -> Result<()> {
    let data = serde_json::to_vec(value)?;
    ensure!(data.len() <= 65536, "Discord frame exceeds 64 KiB");
    writer.write_all(&opcode.to_le_bytes()).await?;
    writer.write_all(&(data.len() as u32).to_le_bytes()).await?;
    writer.write_all(&data).await?;
    writer.flush().await?;
    Ok(())
}
async fn receive(reader: &mut (impl AsyncRead + Unpin + ?Sized)) -> Result<(u32, Value)> {
    let opcode = reader.read_u32_le().await?;
    let length = reader.read_u32_le().await? as usize;
    ensure!(length <= 65536, "Discord frame exceeds 64 KiB");
    let mut data = vec![0; length];
    reader.read_exact(&mut data).await?;
    Ok((opcode, serde_json::from_slice(&data)?))
}
fn payload(activity: Option<&Activity>) -> Value {
    let activity=activity.map(|a|{let mut value=json!({"type":2,"details":a.title.chars().take(128).collect::<String>(),"state":if a.artist.is_empty(){a.album.chars().take(128).collect::<String>()}else{a.artist.chars().take(128).collect::<String>()},"timestamps":{"start":a.started}});if let Some(end)=a.ends{value["timestamps"]["end"]=end.into();}value});
    json!({"cmd":"SET_ACTIVITY","args":{"pid":std::process::id(),"activity":activity},"nonce":format!("{}",rand::random::<u64>())})
}
async fn session(app_id: &str, rx: &mut watch::Receiver<Option<Activity>>) -> Result<()> {
    let mut stream = tokio::time::timeout(Duration::from_secs(5), connect()).await??;
    tokio::time::timeout(
        Duration::from_secs(5),
        send(&mut *stream, 0, &json!({"v":1,"client_id":app_id})),
    )
    .await??;
    let (opcode, ready) =
        tokio::time::timeout(Duration::from_secs(5), receive(&mut *stream)).await??;
    ensure!(
        opcode == 1 && ready["evt"] == "READY",
        "Discord did not accept the handshake"
    );
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (tx, mut messages) = tokio::sync::mpsc::channel(8);
    // Keep frame reads alive across activity updates so partial reads cannot be cancelled.
    let read = AbortRead(tokio::spawn(async move {
        loop {
            let result = receive(&mut reader).await;
            let failed = result.is_err();
            if tx.send(result).await.is_err() || failed {
                break;
            }
        }
    }));
    let result=async{
        loop{
            let activity=rx.borrow_and_update().clone();let value=payload(activity.as_ref());tokio::time::timeout(Duration::from_secs(5),send(&mut writer,1,&value)).await??;
            loop{tokio::select!{
                changed=rx.changed()=>{changed.context("Presence stopped")?;break;},
                message=messages.recv()=>{let (opcode,value)=message.context("Discord disconnected")??;match opcode{2=>bail!("Discord closed the connection"),3=>tokio::time::timeout(Duration::from_secs(5),send(&mut writer,4,&value)).await??,1 if value["evt"]=="ERROR"=>bail!("Discord rejected the activity"),_=>{}}},
            }}
        }
        #[allow(unreachable_code)] Ok::<_,anyhow::Error>(())
    }.await;
    drop(read);
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn frames_round_trip_and_reject_oversized() {
        let (mut a, mut b) = tokio::io::duplex(2048);
        let value = json!({"type":"PING"});
        send(&mut a, 3, &value).await.unwrap();
        assert_eq!(receive(&mut b).await.unwrap(), (3, value));
        a.write_all(&1u32.to_le_bytes()).await.unwrap();
        a.write_all(&65537u32.to_le_bytes()).await.unwrap();
        assert!(receive(&mut b).await.is_err());
    }
    #[test]
    fn payload_contains_only_listening_metadata() {
        let a = Activity {
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            started: 123,
            ends: Some(456),
        };
        let value = payload(Some(&a));
        assert_eq!(value["args"]["activity"]["type"], 2);
        assert_eq!(value["args"]["activity"]["timestamps"]["end"], 456);
        assert!(payload(None)["args"]["activity"].is_null());
    }
}
