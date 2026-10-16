#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[path = "controls_desktop.rs"]
mod desktop;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use desktop::*;
#[cfg(any(target_os = "android", target_os = "ios"))]
mod mobile {
    use std::time::Duration;
    #[allow(dead_code)]
    #[derive(PartialEq, Eq)]
    pub enum SeekDirection {
        Forward,
        Backward,
    }
    pub struct MediaPosition(pub Duration);
    #[allow(dead_code)]
    pub enum MediaControlEvent {
        Play,
        Pause,
        Toggle,
        Next,
        Previous,
        Stop,
        SetVolume(f64),
        SetPosition(MediaPosition),
        Seek(SeekDirection),
        SeekBy(SeekDirection, Duration),
        OpenUri(String),
        Quit,
        Raise,
    }
    pub fn init(_: &slint::Window, _: impl Fn(MediaControlEvent) + Send + 'static) {}
    pub fn update(
        media: Option<&auvemir_core::model::Media>,
        playing: bool,
        position: u64,
        volume: f32,
        speed: f32,
        can_seek: bool,
    ) {
        crate::platform::now_playing(media, playing, position, volume, speed, can_seek);
    }
    pub fn shutdown() {}
}
#[cfg(any(target_os = "android", target_os = "ios"))]
pub use mobile::*;
