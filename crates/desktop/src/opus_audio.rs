//! Ogg Opus, mapping family 0 (mono/stereo), decoded incrementally through libopus.
use anyhow::{Context, Result, bail};
use rodio::Source;
use std::{
    io::{Read, Seek},
    num::{NonZeroU16, NonZeroU32},
    time::Duration,
};

pub struct OpusAudio<R: Read + Seek> {
    reader: ogg::reading::PacketReader<R>,
    decoder: opus::Decoder,
    channels: u16,
    serial: u32,
    skip: usize,
    decoded_frames: u64,
    emitted: u64,
    gain: f32,
    buffer: Vec<f32>,
    index: usize,
    done: bool,
}
impl<R: Read + Seek> OpusAudio<R> {
    pub fn open(reader: R) -> Result<Self> {
        let mut reader = ogg::reading::PacketReader::new(reader);
        let head = reader.read_packet()?.context("Missing Opus header")?;
        let data = &head.data;
        if data.len() < 19 || &data[..8] != b"OpusHead" || data[8] > 15 {
            bail!("Not a supported Ogg Opus stream");
        }
        let channels = data[9] as u16;
        if !matches!(channels, 1 | 2) || data[18] != 0 {
            bail!("This Opus implementation supports mono and stereo mapping family 0");
        }
        let skip = u16::from_le_bytes([data[10], data[11]]) as usize * channels as usize;
        let gain = i16::from_le_bytes([data[16], data[17]]) as f32 / 256.0;
        let decoder = opus::Decoder::new(
            48_000,
            if channels == 1 {
                opus::Channels::Mono
            } else {
                opus::Channels::Stereo
            },
        )?;
        let tags = reader.read_packet()?.context("Missing Opus tags")?;
        if !tags.data.starts_with(b"OpusTags") {
            bail!("Missing Opus comment header");
        }
        Ok(Self {
            reader,
            decoder,
            channels,
            serial: head.stream_serial(),
            skip,
            decoded_frames: 0,
            emitted: 0,
            gain: 10f32.powf(gain / 20.0),
            buffer: vec![],
            index: 0,
            done: false,
        })
    }
    fn packet(&mut self) -> Result<bool> {
        let Some(packet) = self.reader.read_packet()? else {
            return Ok(false);
        };
        if packet.stream_serial() != self.serial {
            bail!("Chained or multiplexed Opus streams are not supported");
        }
        if packet.data.len() > 65_536 {
            bail!("Opus packet is too large");
        }
        self.buffer.resize(5760 * self.channels as usize, 0.0);
        let frames = self
            .decoder
            .decode_float(&packet.data, &mut self.buffer, false)?;
        let usable = if packet.last_in_stream() {
            packet
                .absgp_page()
                .saturating_sub(self.decoded_frames)
                .min(frames as u64) as usize
        } else {
            frames
        };
        self.decoded_frames += frames as u64;
        self.buffer.truncate(usable * self.channels as usize);
        self.index = self.skip.min(self.buffer.len());
        self.skip -= self.index;
        for s in &mut self.buffer {
            *s = (*s * self.gain).clamp(-1.0, 1.0);
        }
        Ok(true)
    }
}
impl<R: Read + Seek> Iterator for OpusAudio<R> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        while self.index >= self.buffer.len() {
            if self.done {
                return None;
            }
            match self.packet() {
                Ok(true) => {}
                Ok(false) => {
                    self.done = true;
                    return None;
                }
                Err(e) => {
                    tracing::warn!("Opus decode failed: {e}");
                    self.done = true;
                    return None;
                }
            }
        }
        let sample = self.buffer[self.index];
        self.index += 1;
        self.emitted += 1;
        Some(sample)
    }
}
impl<R: Read + Seek> Source for OpusAudio<R> {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.buffer.len().saturating_sub(self.index))
    }
    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(self.channels).unwrap()
    }
    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(48000).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        let target = (pos.as_secs_f64() * 48000.0 * self.channels as f64) as u64;
        if target < self.emitted {
            return Err(rodio::source::SeekError::NotSupported {
                underlying_source: "Ogg Opus backward seek",
            });
        }
        // The controller reopens the file first; seeking is linear and occurs off the UI/audio thread.
        for _ in self.emitted..target {
            if self.next().is_none() {
                break;
            }
        }
        Ok(())
    }
}
