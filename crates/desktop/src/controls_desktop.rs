use auvemir_core::model::Media;
use souvlaki::{MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};
use std::{cell::RefCell, time::Duration};
thread_local! {static CONTROLS:RefCell<Option<MediaControls>>=const{RefCell::new(None)};}
thread_local! {static LAST_MEDIA:RefCell<Option<Media>>=const{RefCell::new(None)};}

pub fn init(
    window: &slint::Window,
    handler: impl Fn(souvlaki::MediaControlEvent) + Send + 'static,
) {
    #[cfg(target_os = "linux")]
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        return;
    }
    #[cfg(target_os = "linux")]
    if zbus::blocking::Connection::session().is_err() {
        tracing::warn!("System media controls could not connect to the session bus");
        return;
    }
    #[cfg(target_os = "windows")]
    let hwnd = {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use slint::winit_030::WinitWindowAccessor;
        window
            .with_winit_window(|w| match w.window_handle().map(|h| h.as_raw()) {
                Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get() as *mut std::ffi::c_void),
                _ => None,
            })
            .flatten()
    };
    #[cfg(not(target_os = "windows"))]
    let hwnd = None;
    let _ = window;
    let result = (|| {
        let mut c = MediaControls::new(PlatformConfig {
            display_name: "Auvemir",
            dbus_name: "auvemir",
            hwnd,
        })?;
        c.attach(handler)?;
        Ok::<_, souvlaki::Error>(c)
    })();
    match result {
        Ok(c) => CONTROLS.with(|slot| *slot.borrow_mut() = Some(c)),
        Err(e) => tracing::warn!("System media controls are unavailable: {e}"),
    }
}
pub fn update(
    media: Option<&Media>,
    playing: bool,
    position: u64,
    volume: f32,
    _speed: f32,
    _can_seek: bool,
) {
    CONTROLS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(c) = slot.as_mut() else { return };
        let progress = Some(MediaPosition(Duration::from_millis(position)));
        let playback = if media.is_none() {
            MediaPlayback::Stopped
        } else if playing {
            MediaPlayback::Playing { progress }
        } else {
            MediaPlayback::Paused { progress }
        };
        let _ = c.set_playback(playback);
        #[cfg(target_os = "linux")]
        let _ = c.set_volume(volume as f64);
        let _ = volume;
        let changed = LAST_MEDIA.with(|last| last.borrow().as_ref() != media);
        if let Some(m) = media
            && changed
        {
            // Remote image fetching is managed by the app, never implicitly by the OS adapter.
            let cover = m
                .artwork
                .as_deref()
                .filter(|p| !p.starts_with("http"))
                .and_then(|p| url::Url::from_file_path(p).ok())
                .map(|u| u.to_string());
            let _ = c.set_metadata(MediaMetadata {
                title: Some(&m.title),
                artist: Some(&m.artist),
                album: Some(&m.album),
                cover_url: cover.as_deref(),
                duration: (m.duration_ms > 0).then(|| Duration::from_millis(m.duration_ms)),
            });
        }
        if changed {
            LAST_MEDIA.with(|last| *last.borrow_mut() = media.cloned());
        }
    });
}
pub fn shutdown() {
    CONTROLS.with(|c| {
        c.borrow_mut().take();
    });
}

pub use souvlaki::{MediaControlEvent, MediaPosition, SeekDirection};
