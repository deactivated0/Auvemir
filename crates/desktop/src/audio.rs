//! Decoders and network readers run outside the audio callback. Both encoded
//! streaming storage and decoded PCM queues have fixed limits.
use anyhow::{Context, Result};
use auvemir_core::model::Media;
use crossbeam_channel::{Receiver, TryRecvError};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::{
    collections::HashMap,
    fs::File,
    num::{NonZeroU16, NonZeroU32, NonZeroUsize},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use stream_download::{
    Settings, StreamDownload,
    http::HttpStream,
    storage::{bounded::BoundedStorageProvider, memory::MemoryStorageProvider},
};

pub struct Prepared {
    pub source: Box<dyn Source + Send>,
    pub duration_ms: u64,
    pub settings: Arc<crate::dsp::Settings>,
}

pub async fn prepare(media: Media, offset_ms: u64, looping: bool) -> Result<Prepared> {
    prepare_config(
        media,
        offset_ms,
        looping,
        None,
        Arc::new(crate::dsp::Settings::default()),
    )
    .await
}
pub async fn prepare_config(
    media: Media,
    offset_ms: u64,
    looping: bool,
    custom_client: Option<reqwest::Client>,
    settings: Arc<crate::dsp::Settings>,
) -> Result<Prepared> {
    if media.is_remote() {
        let local = media.provider == "custom-radio" || custom_client.is_some();
        let url = auvemir_core::network::validate_url(&media.location, local)?;
        let client = match custom_client {
            Some(c) => c,
            None => auvemir_core::network::client(local)?,
        };
        let stream = HttpStream::new(client, url).await?;
        let reader = StreamDownload::from_stream(
            stream,
            BoundedStorageProvider::new(
                MemoryStorageProvider,
                NonZeroUsize::new(1024 * 1024).unwrap(),
            ),
            Settings::default().prefetch_bytes(64 * 1024),
        )
        .await?;
        tokio::task::spawn_blocking(move || {
            use std::io::BufRead;
            let mut reader=std::io::BufReader::new(reader);
            let header=reader.fill_buf()?;
            let decoder:Box<dyn Source+Send>=if header.starts_with(b"OggS") && header.windows(8).take(128).any(|s|s==b"OpusHead") {
                Box::new(crate::opus_audio::OpusAudio::open(reader)?)
            }else{Box::new(Decoder::builder().with_data(reader).with_seekable(false).build().context("Stream codec is unsupported or the URL is a playlist rather than audio")?)};
            {let mut p=buffer(decoder,0,false,settings)?;if p.duration_ms==0{p.duration_ms=media.duration_ms;}Ok(p)}
        }).await?
    } else {
        tokio::task::spawn_blocking(move || {
            let decoder = decode_file(std::path::Path::new(&media.location))?;
            let mut prepared = buffer(decoder, offset_ms, looping, settings)?;
            if prepared.duration_ms == 0 {
                prepared.duration_ms = media.duration_ms;
            }
            Ok(prepared)
        })
        .await?
    }
}

fn buffer(
    mut decoder: impl Source + Send + 'static,
    offset_ms: u64,
    looping: bool,
    settings: Arc<crate::dsp::Settings>,
) -> Result<Prepared> {
    let channels = decoder.channels();
    let rate = decoder.sample_rate();
    let duration_ms = decoder
        .total_duration()
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if offset_ms > 0 {
        decoder.try_seek(Duration::from_millis(offset_ms))?;
    }
    let source: Box<dyn Source + Send> = if looping {
        Box::new(decoder.repeat_infinite())
    } else {
        Box::new(decoder)
    };
    let (tx, rx) = crossbeam_channel::bounded::<Vec<f32>>(12);
    let (recycle_tx, recycle_rx) = crossbeam_channel::bounded::<Vec<f32>>(14);
    let chunk_size = 2048 * channels.get() as usize;
    for _ in 0..14 {
        recycle_tx.send(Vec::with_capacity(chunk_size))?;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let worker_settings = settings.clone();
    let pool_tx = recycle_tx.clone();
    std::thread::Builder::new()
        .name("auvemir-decode".into())
        .spawn(move || {
            let mut source = source;
            let mut processor = crate::dsp::Processor::new(channels.get() as usize, rate.get());
            let mut input = Vec::with_capacity(chunk_size);
            let mut finished = false;
            while !worker_cancel.load(Ordering::Relaxed) {
                input.clear();
                if !finished {
                    input.extend(source.by_ref().take(chunk_size));
                    if input.is_empty() {
                        finished = true;
                        processor.flush();
                    } else {
                        processor.process(&mut input, &worker_settings);
                    }
                }
                loop {
                    let mut output = loop {
                        match recycle_rx.recv_timeout(Duration::from_millis(50)) {
                            Ok(v) => break v,
                            Err(_) => {
                                if worker_cancel.load(Ordering::Relaxed) {
                                    return;
                                }
                            }
                        }
                    };
                    output.clear();
                    if processor.stretched() {
                        output.resize(chunk_size, 0.);
                        let n = processor.receive(&mut output);
                        output.truncate(n);
                    } else {
                        output.extend_from_slice(&input);
                    }
                    if output.is_empty() {
                        let _ = pool_tx.try_send(output);
                        if finished {
                            return;
                        }
                        break;
                    }
                    let mut pending = output;
                    loop {
                        match tx.send_timeout(pending, Duration::from_millis(50)) {
                            Ok(()) => break,
                            Err(crossbeam_channel::SendTimeoutError::Timeout(v)) => {
                                if worker_cancel.load(Ordering::Relaxed) {
                                    return;
                                }
                                pending = v;
                            }
                            Err(_) => return,
                        }
                    }
                    if !processor.stretched() {
                        break;
                    }
                }
            }
        })?;
    // Prime before attaching to the output mixer. No blocking reads occur in next().
    let first = rx
        .recv_timeout(Duration::from_secs(12))
        .context("Decoder produced no audio")?;
    Ok(Prepared {
        source: Box::new(BufferedAudio {
            rx,
            recycle: recycle_tx,
            chunk: first,
            index: 0,
            channels,
            rate,
            cancel,
            underruns: Arc::new(AtomicU64::new(0)),
        }),
        duration_ms,
        settings,
    })
}

struct BufferedAudio {
    rx: Receiver<Vec<f32>>,
    recycle: crossbeam_channel::Sender<Vec<f32>>,
    chunk: Vec<f32>,
    index: usize,
    channels: NonZeroU16,
    rate: NonZeroU32,
    cancel: Arc<AtomicBool>,
    underruns: Arc<AtomicU64>,
}
impl Iterator for BufferedAudio {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.index == self.chunk.len() {
            match self.rx.try_recv() {
                Ok(c) => {
                    let previous = std::mem::replace(&mut self.chunk, c);
                    let _ = self.recycle.try_send(previous);
                    self.index = 0;
                }
                Err(TryRecvError::Disconnected) => return None,
                Err(TryRecvError::Empty) => {
                    self.underruns.fetch_add(1, Ordering::Relaxed);
                    return Some(0.0);
                }
            }
        }
        let sample = self.chunk[self.index];
        self.index += 1;
        Some(sample)
    }
}
impl Source for BufferedAudio {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.chunk.len().saturating_sub(self.index).max(1))
    }
    fn channels(&self) -> NonZeroU16 {
        self.channels
    }
    fn sample_rate(&self) -> NonZeroU32 {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
impl Drop for BufferedAudio {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub struct Audio {
    device: Option<MixerDeviceSink>,
    pub player: Option<Player>,
    pub duration_ms: u64,
    pub offset_ms: u64,
    pub volume: f32,
    pub speed: f32,
    layers: HashMap<String, Player>,
    layer_volumes: HashMap<String, f32>,
    pub dsp: Arc<crate::dsp::Settings>,
    active_dsp: Option<Arc<crate::dsp::Settings>>,
    speed_origin: u64,
}
impl Audio {
    pub fn new() -> Self {
        Self {
            device: None,
            player: None,
            duration_ms: 0,
            offset_ms: 0,
            volume: 0.8,
            speed: 1.0,
            layers: HashMap::new(),
            layer_volumes: HashMap::new(),
            dsp: Arc::new(crate::dsp::Settings::default()),
            active_dsp: None,
            speed_origin: 0,
        }
    }
    fn mixer(&mut self) -> Result<&rodio::mixer::Mixer> {
        if self.device.is_none() {
            self.device = Some(
                DeviceSinkBuilder::open_default_sink().context("No usable audio output device")?,
            );
        }
        Ok(self.device.as_ref().unwrap().mixer())
    }
    pub fn play(&mut self, p: Prepared, offset_ms: u64) -> Result<()> {
        self.play_state(p, offset_ms, true)
    }
    pub fn play_state(&mut self, p: Prepared, offset_ms: u64, playing: bool) -> Result<()> {
        let player = Player::connect_new(self.mixer()?);
        if !playing {
            player.pause();
        }
        player.set_volume(self.volume);
        p.settings.set_tempo(self.speed);
        for (i, g) in self.dsp.gains().iter().enumerate() {
            p.settings.set_gain(i, *g);
        }
        self.active_dsp = Some(p.settings.clone());
        player.append(p.source);
        self.stop();
        self.player = Some(player);
        self.duration_ms = p.duration_ms;
        self.offset_ms = offset_ms;
        self.speed_origin = 0;
        Ok(())
    }
    pub fn toggle(&self) {
        if let Some(p) = &self.player {
            if p.is_paused() { p.play() } else { p.pause() }
        }
    }
    pub fn pause(&self) {
        if let Some(p) = &self.player {
            p.pause();
        }
    }
    pub fn resume(&self) {
        if let Some(p) = &self.player {
            p.play();
        }
    }
    pub fn stop(&mut self) {
        self.offset_ms = self.position();
        if let Some(p) = self.player.take() {
            p.stop();
        }
    }
    pub fn stop_all(&mut self) {
        self.stop();
        for (_, p) in self.layers.drain() {
            p.stop();
        }
    }
    pub fn playing(&self) -> bool {
        self.player
            .as_ref()
            .is_some_and(|p| !p.is_paused() && !p.empty())
    }
    pub fn ended(&self) -> bool {
        self.player.as_ref().is_some_and(|p| p.empty())
    }
    pub fn position(&self) -> u64 {
        self.offset_ms
            + self
                .player
                .as_ref()
                .map(|p| {
                    ((p.get_pos().as_millis() as u64).saturating_sub(self.speed_origin) as f64
                        * self.speed as f64) as u64
                })
                .unwrap_or(0)
    }
    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        if let Some(p) = &self.player {
            p.set_volume(self.volume);
        }
        for (id, p) in &self.layers {
            p.set_volume(self.volume * self.layer_volumes.get(id).copied().unwrap_or(0.25));
        }
    }
    pub fn set_speed(&mut self, v: f32) {
        self.offset_ms = self.position();
        self.speed_origin = self
            .player
            .as_ref()
            .map(|p| p.get_pos().as_millis() as u64)
            .unwrap_or(0);
        self.speed = v.clamp(0.5, 2.0);
        self.dsp.set_tempo(self.speed);
        if let Some(settings) = &self.active_dsp {
            settings.set_tempo(self.speed);
        }
    }
    pub fn layer_volume(&mut self, id: &str, value: f32) {
        self.layer_volumes.insert(id.into(), value.clamp(0., 1.));
        if let Some(p) = self.layers.get(id) {
            p.set_volume(self.volume * value.clamp(0., 1.));
        }
    }
    pub fn layer_ids(&self) -> Vec<String> {
        self.layers.keys().cloned().collect()
    }
    pub fn layer_gain(&self, id: &str) -> f32 {
        self.layer_volumes.get(id).copied().unwrap_or(0.25)
    }
    pub fn has_layer(&self, id: &str) -> bool {
        self.layers.contains_key(id)
    }
    pub fn has_layers(&self) -> bool {
        !self.layers.is_empty()
    }
    pub fn layers_playing(&self) -> bool {
        self.layers.values().any(|p| !p.is_paused() && !p.empty())
    }
    pub fn pause_layers(&self) {
        for player in self.layers.values() {
            player.pause();
        }
    }
    pub fn resume_layers(&self) {
        for player in self.layers.values() {
            player.play();
        }
    }
    pub fn stop_layers(&mut self) {
        for (_, p) in self.layers.drain() {
            p.stop();
        }
    }
    pub fn eq(&mut self, index: usize, value: f32) {
        self.dsp.set_gain(index, value);
        if let Some(active) = &self.active_dsp {
            active.set_gain(index, value);
        }
    }
    pub fn layer(&mut self, id: String, p: Prepared, playing: bool) -> Result<()> {
        if let Some(old) = self.layers.remove(&id) {
            old.stop();
            return Ok(());
        }
        let player = Player::connect_new(self.mixer()?);
        if !playing {
            player.pause();
        }
        player.set_volume(self.volume * self.layer_volumes.get(&id).copied().unwrap_or(0.25));
        player.append(p.source);
        self.layers.insert(id, player);
        Ok(())
    }
    pub fn noise(&mut self, kind: &str) -> Result<bool> {
        if let Some(p) = self.layers.remove(kind) {
            p.stop();
            return Ok(false);
        }
        let player = Player::connect_new(self.mixer()?);
        player.set_volume(self.volume * self.layer_volumes.get(kind).copied().unwrap_or(0.25));
        player.append(Noise {
            seed: 0xa1b2c3d4,
            kind: kind.to_owned(),
            brown: 0.0,
            pink: 0.0,
        });
        self.layers.insert(kind.to_owned(), player);
        Ok(true)
    }
}
struct Noise {
    seed: u64,
    kind: String,
    brown: f32,
    pink: f32,
}
impl Iterator for Noise {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        let white = ((self.seed >> 40) as f32 / 8388608.0) - 1.0;
        self.brown = (self.brown + white * 0.02).clamp(-0.8, 0.8) * 0.998;
        self.pink = 0.97 * self.pink + 0.03 * white;
        Some(
            match self.kind.as_str() {
                "brown" => self.brown,
                "pink" => self.pink * 4.0,
                _ => white * 0.35,
            }
            .clamp(-1.0, 1.0),
        )
    }
}
impl Source for Noise {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(1).unwrap()
    }
    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(44100).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Content probing remains primary; the filename is only a hint.
pub fn decode_file(path: &std::path::Path) -> Result<Box<dyn Source + Send>> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut builder = Decoder::builder()
        .with_data(file)
        .with_byte_len(len)
        .with_seekable(true);
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        builder = builder.with_hint(ext);
    }
    match builder.build() {
        Ok(d) => Ok(Box::new(d)),
        Err(original) => {
            match crate::opus_audio::OpusAudio::open(std::io::BufReader::new(File::open(path)?)) {
                Ok(d) => Ok(Box::new(d)),
                Err(_) => {
                    let _ = original;
                    Ok(Box::new(crate::ffmpeg_audio::FfmpegAudio::open(path)?))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn noise_is_bounded_and_non_silent() {
        for kind in ["white", "pink", "brown"] {
            let mut n = Noise {
                seed: 42,
                kind: kind.into(),
                brown: 0.0,
                pink: 0.0,
            };
            let samples: Vec<_> = n.by_ref().take(44100).collect();
            assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
            assert!(samples.iter().any(|s| s.abs() > 0.01));
        }
    }
}
