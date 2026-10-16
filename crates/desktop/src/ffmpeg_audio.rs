//! Optional extended-codec decoder. Only invoked after native decoding fails.
use anyhow::{Context, Result};
use rodio::Source;
use std::{
    io::{BufReader, Read},
    num::{NonZeroU16, NonZeroU32},
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    time::Duration,
};
pub struct FfmpegAudio {
    child: Child,
    reader: BufReader<ChildStdout>,
    path: PathBuf,
    duration: Option<Duration>,
    program: String,
}
impl FfmpegAudio {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path.canonicalize()?;
        let program = std::env::var("AUVEMIR_FFMPEG").unwrap_or_else(|_| "ffmpeg".into());
        let duration =
            auvemir_core::library::read_media(&path, auvemir_core::model::MediaKind::LocalMusic)
                .ok()
                .filter(|m| m.duration_ms > 0)
                .map(|m| Duration::from_millis(m.duration_ms));
        let (child, reader) = spawn(&program, &path, Duration::ZERO)?;
        Ok(Self {
            child,
            reader,
            path,
            duration,
            program,
        })
    }
}
fn spawn(program: &str, path: &Path, offset: Duration) -> Result<(Child, BufReader<ChildStdout>)> {
    let mut child = Command::new(program)
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-ss",
            &format!("{:.3}", offset.as_secs_f64()),
            "-i",
        ])
        .arg(path)
        .args([
            "-vn",
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-ac",
            "2",
            "-ar",
            "48000",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("Native codec is unsupported. Install FFmpeg to enable the optional decoder")?;
    let reader = BufReader::with_capacity(
        65536,
        child.stdout.take().context("Cannot read FFmpeg output")?,
    );
    Ok((child, reader))
}
impl Iterator for FfmpegAudio {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let mut bytes = [0u8; 4];
        self.reader.read_exact(&mut bytes).ok()?;
        let sample = f32::from_le_bytes(bytes);
        Some(if sample.is_finite() {
            sample.clamp(-1., 1.)
        } else {
            0.
        })
    }
}
impl Source for FfmpegAudio {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(2).unwrap()
    }
    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(48000).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.duration
    }
    fn try_seek(&mut self, pos: Duration) -> std::result::Result<(), rodio::source::SeekError> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        match spawn(&self.program, &self.path, pos) {
            Ok((child, reader)) => {
                self.child = child;
                self.reader = reader;
                Ok(())
            }
            Err(e) => Err(rodio::source::SeekError::Other(std::sync::Arc::new(
                std::io::Error::other(e.to_string()),
            ))),
        }
    }
}
impl Drop for FfmpegAudio {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
