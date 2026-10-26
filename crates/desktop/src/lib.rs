mod app;
mod artwork;
mod audio;
mod capture;
mod casting;
mod controls;
mod credentials;
mod dsp;
mod ffmpeg_audio;
mod fingerprint;
mod motion;
mod opus_audio;
mod recorder;
mod rooms;
mod transport;
mod ui_target;
mod wake;

pub fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(format!(
            "{},stream_download=off,reqwest=off,hyper=off",
            std::env::var("RUST_LOG").unwrap_or_else(|_| "auvemir=info".into())
        )))
        .try_init()
        .ok();
    app::run()
}

#[cfg(target_os = "android")]
mod mobile_android;
#[cfg(target_os = "ios")]
mod mobile_ios;
mod platform;
