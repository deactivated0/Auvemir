use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{Receiver, Sender};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    thread::JoinHandle,
};

#[derive(Clone)]
struct Capture {
    tx: Sender<Vec<i16>>,
    pool: Receiver<Vec<i16>>,
    recycle: Sender<Vec<i16>>,
    dropped: Arc<AtomicU64>,
    level: Arc<AtomicU32>,
}
impl Capture {
    fn samples<T>(&self, data: &[T], convert: impl Fn(&T) -> i16) {
        capture(
            data,
            &self.tx,
            &self.pool,
            &self.recycle,
            &self.dropped,
            &self.level,
            convert,
        );
    }
}
struct Input {
    device: cpal::Device,
    config: cpal::StreamConfig,
    format: cpal::SampleFormat,
    capture: Capture,
}
impl Input {
    fn open(&self) -> Result<cpal::Stream> {
        let capture = self.capture.clone();
        let error = |err| tracing::error!("Microphone error: {err}");
        Ok(match self.format {
            cpal::SampleFormat::F32 => self.device.build_input_stream(
                self.config,
                move |data: &[f32], _| {
                    capture.samples(data, |s| (s.clamp(-1., 1.) * i16::MAX as f32) as i16)
                },
                error,
                None,
            )?,
            cpal::SampleFormat::I16 => self.device.build_input_stream(
                self.config,
                move |data: &[i16], _| capture.samples(data, |s| *s),
                error,
                None,
            )?,
            cpal::SampleFormat::U16 => self.device.build_input_stream(
                self.config,
                move |data: &[u16], _| capture.samples(data, |s| (*s as i32 - 32768) as i16),
                error,
                None,
            )?,
            _ => bail!("Microphone sample format is not supported"),
        })
    }
}
pub struct Recorder {
    _lock: Arc<std::fs::File>,
    stream: Option<cpal::Stream>,
    input: Option<Input>,
    writer: Option<JoinHandle<Result<()>>>,
    pub path: PathBuf,
    pub dropped: Arc<AtomicU64>,
    pub level: Arc<AtomicU32>,
    pub paused: bool,
}
impl Recorder {
    pub fn start(dir: &Path, notify: impl FnOnce() + Send + 'static) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let lock = Arc::new(recording_lock(dir)?);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        let path = dir.join(format!("Voice {stamp}.wav"));
        let partial = path.with_extension("wav.part");
        let device = cpal::default_host()
            .default_input_device()
            .context("No microphone available")?;
        let supported = device.default_input_config()?;
        let config: cpal::StreamConfig = supported.into();
        let spec = hound::WavSpec {
            channels: config.channels,
            sample_rate: config.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let wav = hound::WavWriter::create(&partial, spec)?;
        let (tx, rx) = crossbeam_channel::bounded(64);
        let dropped = Arc::new(AtomicU64::new(0));
        let level = Arc::new(AtomicU32::new(0));
        let (pool_tx, pool_rx) = crossbeam_channel::bounded(64);
        for _ in 0..64 {
            pool_tx.send(Vec::<i16>::with_capacity(4096))?;
        }
        let input = Input {
            device,
            config,
            format: supported.sample_format(),
            capture: Capture {
                tx,
                pool: pool_rx,
                recycle: pool_tx.clone(),
                dropped: dropped.clone(),
                level: level.clone(),
            },
        };
        let stream = input.open()?;
        stream.play()?;
        let writer = writer_thread(
            wav,
            rx,
            pool_tx,
            partial,
            path.clone(),
            lock.clone(),
            notify,
        )?;
        Ok(Self {
            _lock: lock,
            stream: Some(stream),
            input: Some(input),
            writer: Some(writer),
            path,
            dropped,
            level,
            paused: false,
        })
    }
    pub fn toggle_pause(&mut self) -> Result<()> {
        if self.paused {
            if self.stream.is_none() {
                self.stream = Some(
                    self.input
                        .as_ref()
                        .context("Microphone is closed")?
                        .open()?,
                );
            }
            self.stream.as_ref().unwrap().play()?;
        } else if let Err(error) = self
            .stream
            .as_ref()
            .context("Microphone is closed")?
            .pause()
        {
            if error.kind() == cpal::ErrorKind::UnsupportedOperation {
                // AAudio input cannot pause. Closing releases the microphone; the writer and
                // reusable buffers remain alive so resume appends to the same WAV timeline.
                self.stream.take();
            } else {
                return Err(error.into());
            }
        }
        self.paused = !self.paused;
        Ok(())
    }
    pub fn writer_finished(&self) -> bool {
        self.writer
            .as_ref()
            .is_some_and(|writer| writer.is_finished())
    }
    pub fn stop(mut self) -> Result<(PathBuf, u64)> {
        if let Some(stream) = self.stream.take() {
            let _ = stream.pause();
            drop(stream);
        }
        self.input.take(); // Release the last producer before joining the draining writer.
        self.writer
            .take()
            .unwrap()
            .join()
            .map_err(|_| anyhow::anyhow!("Recording writer failed"))??;
        Ok((self.path, self.dropped.load(Ordering::Relaxed)))
    }
}
struct Notify<F: FnOnce()>(Option<F>);
impl<F: FnOnce()> Drop for Notify<F> {
    fn drop(&mut self) {
        if let Some(notify) = self.0.take() {
            notify();
        }
    }
}
fn writer_thread(
    wav: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    rx: Receiver<Vec<i16>>,
    pool: Sender<Vec<i16>>,
    partial: PathBuf,
    final_path: PathBuf,
    lock: Arc<std::fs::File>,
    notify: impl FnOnce() + Send + 'static,
) -> Result<JoinHandle<Result<()>>> {
    Ok(std::thread::Builder::new()
        .name("auvemir-record".into())
        .spawn(move || {
            let _notify = Notify(Some(notify));
            let _lock = lock; // Survives Recorder teardown and stays held until the file is closed.
            let mut wav = wav; // Drops before _lock on every error or unwind path.
            let mut flushed = std::time::Instant::now();
            for mut chunk in rx {
                for sample in &chunk {
                    wav.write_sample(*sample)?;
                }
                chunk.clear();
                let _ = pool.try_send(chunk);
                if flushed.elapsed() >= std::time::Duration::from_secs(2) {
                    wav.flush()?;
                    flushed = std::time::Instant::now();
                }
            }
            wav.finalize()?;
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&partial)?
                .sync_all()?;
            std::fs::rename(partial, final_path)?;
            Ok(())
        })?)
}
fn recording_lock(dir: &Path) -> Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("capture.lock"))?;
    fs2_lock(&file)?;
    Ok(file)
}
fn fs2_lock(file: &std::fs::File) -> Result<()> {
    use auvemir_core::library::capture_lock;
    capture_lock(file).context("Another Auvemir recorder is active")
}
/// Repair only unfinished files created by Auvemir's PCM16 recorder. An exclusive
/// lock prevents recovery from touching an active recorder in another process.
pub fn recover(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let Ok(_lock) = recording_lock(dir) else {
        return Ok(Vec::new());
    };
    let mut recovered = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_file()
            || !path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with("Voice ") && s.ends_with(".wav.part"))
        {
            continue;
        }
        if let Ok(final_path) = repair(&path) {
            recovered.push(final_path);
        }
    }
    Ok(recovered)
}
fn repair(path: &Path) -> Result<PathBuf> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let length = file.metadata()?.len();
    let mut riff = [0u8; 12];
    file.read_exact(&mut riff)?;
    if &riff[..4] != b"RIFF" || &riff[8..] != b"WAVE" {
        bail!("Not a recorder WAV file");
    }
    let mut at = 12u64;
    let mut frame = None;
    let (header, data) = loop {
        if at > 1024 * 1024 || at + 8 > length {
            bail!("Recording header is invalid");
        }
        file.seek(SeekFrom::Start(at))?;
        let mut chunk = [0u8; 8];
        file.read_exact(&mut chunk)?;
        let size = u32::from_le_bytes(chunk[4..].try_into().unwrap()) as u64;
        if &chunk[..4] == b"fmt " {
            if !(16..=65536).contains(&size) {
                bail!("Unsupported recording format");
            }
            let mut format = [0u8; 16];
            file.read_exact(&mut format)?;
            let channels = u16::from_le_bytes(format[2..4].try_into().unwrap());
            let alignment = u16::from_le_bytes(format[12..14].try_into().unwrap());
            if u16::from_le_bytes(format[..2].try_into().unwrap()) != 1
                || u16::from_le_bytes(format[14..16].try_into().unwrap()) != 16
                || channels == 0
                || channels > 32
                || alignment != channels * 2
            {
                bail!("Unsupported recording format");
            }
            frame = Some(alignment as u64);
        }
        if &chunk[..4] == b"data" {
            break (at, at + 8);
        }
        at = at
            .checked_add(8 + size + (size & 1))
            .context("Invalid recording chunk")?;
    };
    let frame = frame.context("Recording format is missing")?;
    let bytes = (length - data) / frame * frame;
    if bytes == 0 || bytes > u32::MAX as u64 || data + bytes - 8 > u32::MAX as u64 {
        bail!("Recording has no recoverable samples or exceeds WAV limits");
    }
    file.set_len(data + bytes)?;
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&((data + bytes - 8) as u32).to_le_bytes())?;
    file.seek(SeekFrom::Start(header + 4))?;
    file.write_all(&(bytes as u32).to_le_bytes())?;
    file.sync_all()?;
    drop(file);
    let mut target = path.with_extension("");
    if target.exists() {
        target = path.with_file_name(format!("Recovered {:016x}.wav", rand::random::<u64>()));
    }
    std::fs::rename(path, &target)?;
    Ok(target)
}

fn capture<T>(
    data: &[T],
    tx: &Sender<Vec<i16>>,
    pool: &Receiver<Vec<i16>>,
    recycle: &Sender<Vec<i16>>,
    dropped: &AtomicU64,
    peak: &AtomicU32,
    convert: impl Fn(&T) -> i16,
) {
    let mut max = 0f32;
    for chunk in data.chunks(4096) {
        let Ok(mut buffer) = pool.try_recv() else {
            dropped.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            continue;
        };
        buffer.clear();
        for sample in chunk {
            let s = convert(sample);
            max = max.max(s.unsigned_abs() as f32 / 32768.);
            buffer.push(s);
        }
        if let Err(error) = tx.try_send(buffer) {
            dropped.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            let _ = recycle.try_send(error.into_inner());
        }
    }
    peak.store(max.to_bits(), Ordering::Relaxed);
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn repairs_unfinalized_pcm_without_changing_samples() {
        let dir = tempfile_dir();
        let partial = dir.join("Voice 123.wav.part");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        hound::WavWriter::create(&partial, spec)
            .unwrap()
            .finalize()
            .unwrap();
        let samples: [i16; 5] = [0, 123, -456, 1000, -1000];
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&partial)
            .unwrap();
        for s in samples {
            f.write_all(&s.to_le_bytes()).unwrap();
        }
        f.write_all(&[0x7f]).unwrap();
        drop(f);
        let recovered = recover(&dir).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(
            hound::WavReader::open(&recovered[0])
                .unwrap()
                .into_samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            samples
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn recovery_skips_active_recorder_and_invalid_files() {
        let dir = tempfile_dir();
        let partial = dir.join("Voice broken.wav.part");
        std::fs::write(&partial, b"broken").unwrap();
        let lock = recording_lock(&dir).unwrap();
        assert!(recover(&dir).unwrap().is_empty());
        drop(lock);
        assert!(recover(&dir).unwrap().is_empty());
        assert_eq!(std::fs::read(partial).unwrap(), b"broken");
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn draining_writer_retains_lock_after_controller_is_dropped() {
        let dir = tempfile_dir();
        let partial = dir.join("Voice drain.wav.part");
        let output = dir.join("Voice drain.wav");
        let wav = hound::WavWriter::create(
            &partial,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        let controller = Arc::new(recording_lock(&dir).unwrap());
        let (tx, rx) = crossbeam_channel::bounded(1);
        let (pool, _) = crossbeam_channel::bounded(1);
        let (notify, finished) = crossbeam_channel::bounded(1);
        let writer = writer_thread(
            wav,
            rx,
            pool,
            partial,
            output.clone(),
            controller.clone(),
            move || {
                let _ = notify.try_send(());
            },
        )
        .unwrap();
        drop(controller);
        assert!(
            recording_lock(&dir).is_err(),
            "Recovery must not touch a draining writer"
        );
        tx.send(vec![11, -22, 33]).unwrap();
        drop(tx);
        writer.join().unwrap().unwrap();
        finished
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        let released = recording_lock(&dir).unwrap();
        drop(released);
        assert_eq!(
            hound::WavReader::open(output)
                .unwrap()
                .into_samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            vec![11, -22, 33]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn writer_notifies_on_finalize_failure_and_releases_its_lock() {
        let dir = tempfile_dir();
        let partial = dir.join("Voice error.wav.part");
        let output = dir.join("occupied");
        std::fs::create_dir(&output).unwrap();
        let wav = hound::WavWriter::create(
            &partial,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        let lock = Arc::new(recording_lock(&dir).unwrap());
        let (tx, rx) = crossbeam_channel::bounded(1);
        let (pool, _) = crossbeam_channel::bounded(1);
        let (notify, finished) = crossbeam_channel::bounded(1);
        let writer = writer_thread(wav, rx, pool, partial.clone(), output, lock, move || {
            let _ = notify.try_send(());
        })
        .unwrap();
        tx.send(vec![42]).unwrap();
        drop(tx);
        assert!(writer.join().unwrap().is_err());
        finished
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        assert!(partial.is_file());
        let released = recording_lock(&dir).unwrap();
        drop(released);
        std::fs::remove_dir_all(dir).unwrap();
    }
    fn tempfile_dir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "auvemir-record-test-{:016x}",
            rand::random::<u64>()
        ));
        std::fs::create_dir(&p).unwrap();
        p
    }
}
