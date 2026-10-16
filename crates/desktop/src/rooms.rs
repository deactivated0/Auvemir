use auvemir_core::{listen, model::Media};
use std::{
    net::{SocketAddr, UdpSocket},
    sync::{Arc, Mutex},
};
pub enum Connection {
    Host(Box<listen::Host>),
    Guest { _guest: listen::Guest },
}
pub type Inbox = Arc<Mutex<Option<listen::Event>>>;
#[derive(Default)]
pub struct Rooms {
    pub connection: Option<Connection>,
    pub generation: u64,
    pub following: bool,
    pub invitation: String,
    pub latest: Option<(listen::State, i64)>,
    pub loading: Option<String>,
    pub missing: Option<(String, u64)>,
    pub previous_speed: Option<f32>,
    pub inbox: Option<Inbox>,
    pub cancel: Option<tokio::sync::watch::Sender<bool>>,
    sample: Option<(Option<Media>, u64, bool, u32, u64)>,
}
impl Rooms {
    pub fn leave(&mut self) -> Option<f32> {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(true);
        }
        self.connection = None;
        self.generation = self.generation.wrapping_add(1);
        self.following = false;
        self.invitation.clear();
        self.latest = None;
        self.loading = None;
        self.missing = None;
        self.inbox = None;
        self.sample = None;
        self.previous_speed.take()
    }
    pub fn hosting(&self) -> bool {
        matches!(self.connection, Some(Connection::Host(_)))
    }
    pub fn take_event(&self) -> Option<listen::Event> {
        self.inbox.as_ref()?.lock().ok()?.take()
    }
    pub fn publish(
        &mut self,
        media: Option<&Media>,
        position: u64,
        duration: u64,
        playing: bool,
        speed: f32,
    ) -> anyhow::Result<()> {
        let Some(Connection::Host(host)) = &self.connection else {
            return Ok(());
        };
        if self.sample.as_ref().is_some_and(|(m, p, on, s, d)| {
            m.as_ref() == media
                && *p == position / 1000
                && *on == playing
                && *s == speed.to_bits()
                && *d == duration
        }) {
            return Ok(());
        }
        host.publish(media.map(|m| listen::Playback {
            identity: auvemir_core::sync::identity(m),
            title: m.title.clone(),
            artist: m.artist.clone(),
            position_ms: position,
            duration_ms: duration,
            playing,
            speed,
        }))?;
        self.sample = Some((
            media.cloned(),
            position / 1000,
            playing,
            speed.to_bits(),
            duration,
        ));
        Ok(())
    }
}
pub fn default_address() -> anyhow::Result<SocketAddr> {
    // UDP connect selects the route without transmitting a packet.
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("192.0.2.1:9")?;
    Ok(SocketAddr::new(socket.local_addr()?.ip(), 0))
}
