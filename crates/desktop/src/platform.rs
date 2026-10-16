//! Platform operations are lazy; desktop startup never initializes a mobile runtime.
#[cfg(target_os = "android")]
use crate::mobile_android as native;
#[cfg(target_os = "ios")]
use crate::mobile_ios as native;
use anyhow::{Context, Result};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};
type Handler = Arc<dyn Fn(String, String, i64) + Send + Sync>;
static HANDLER: OnceLock<Mutex<Option<Handler>>> = OnceLock::new();
pub fn register(f: impl Fn(String, String, i64) + Send + Sync + 'static) {
    *HANDLER.get_or_init(Default::default).lock().unwrap() = Some(Arc::new(f));
}
pub fn unregister() {
    if let Some(slot) = HANDLER.get() {
        slot.lock().unwrap().take();
    }
}
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn command(action: String, value: String, n: i64) {
    let f = HANDLER.get().and_then(|s| s.lock().ok()?.clone());
    if let Some(f) = f {
        f(action, value, n);
    }
}
pub fn data_dir() -> Result<PathBuf> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        native::data_dir()
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        std::env::var_os("AUVEMIR_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                directories::ProjectDirs::from("org", "Auvemir", "Auvemir")
                    .map(|d| d.data_local_dir().to_path_buf())
            })
            .context("Cannot determine application data directory")
    }
}
pub async fn pick_audio(page: i32, folder: bool) -> Result<Option<String>> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        native::pick(page, folder, "")?;
        Ok(None)
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = page;
        let dialog = rfd::AsyncFileDialog::new().add_filter(
            "Audio",
            &[
                "mp3", "flac", "wav", "ogg", "opus", "m4a", "m4b", "aac", "aiff", "mp2",
            ],
        );
        let file = if folder {
            dialog.pick_folder().await
        } else {
            dialog.pick_file().await
        };
        Ok(file.map(|f| f.path().to_string_lossy().into_owned()))
    }
}
pub fn microphone_ready() -> Result<bool> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        native::microphone_ready()
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        Ok(true)
    }
}
pub fn activate_audio(recording: bool) -> Result<()> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        native::activate_audio(recording)
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = recording;
        Ok(())
    }
}
pub fn recording(present: bool, paused: bool) {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        native::recording(if !present {
            0
        } else if paused {
            2
        } else {
            1
        });
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = (present, paused);
    }
}
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn now_playing(
    media: Option<&auvemir_core::model::Media>,
    playing: bool,
    position: u64,
    volume: f32,
    speed: f32,
    can_seek: bool,
) {
    native::now_playing(media, playing, position, volume, speed, can_seek);
}
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn credential(action: &str, id: &str, secret: &str) -> Result<String> {
    native::credential(action, id, secret)
}
pub fn shutdown() {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        native::shutdown();
    }
}

pub fn routes() {
    #[cfg(target_os = "ios")]
    {
        crate::mobile_ios::routes();
    }
}

pub fn open_url(input: &str) -> Result<()> {
    let url = auvemir_core::network::validate_url(input, false)?;
    webbrowser::open(url.as_str()).context("Could not open the system browser")
}

pub fn pick_document(page: i32, action: &str) -> Result<()> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        native::pick(page, false, action)
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = (page, action);
        anyhow::bail!("Enter an import path")
    }
}
pub fn export_document(path: &str, name: &str) -> Result<()> {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        native::export_document(path, name)
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let _ = (path, name);
        anyhow::bail!("Enter an export path")
    }
}

/// Explicit user transport cancels automatic focus resumption on Android.
pub fn user_transport() {
    #[cfg(target_os = "android")]
    crate::mobile_android::user_transport();
    #[cfg(target_os = "ios")]
    crate::mobile_ios::user_transport();
}
