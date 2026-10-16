//! AcoustID-compatible Chromaprint using the same native decoder as playback.
use anyhow::{Result, ensure};
use base64::Engine;
use rodio::Source;
pub fn calculate(path: &std::path::Path) -> Result<(u64, String)> {
    let mut source = crate::audio::decode_file(path)?;
    let rate = source.sample_rate().get();
    let channels = source.channels().get();
    let duration = source.total_duration().map(|d| d.as_secs());
    let config = rusty_chromaprint::Configuration::preset_test2();
    let mut printer = rusty_chromaprint::Fingerprinter::new(&config);
    printer.start(rate, channels as u32)?;
    let max = rate as u64 * channels as u64 * 120;
    let mut count = 0u64;
    let mut samples = Vec::with_capacity(8192);
    while count < max {
        samples.clear();
        for sample in source.by_ref().take(((max - count) as usize).min(8192)) {
            samples.push((sample.clamp(-1., 1.) * i16::MAX as f32) as i16);
        }
        if samples.is_empty() {
            break;
        }
        count += samples.len() as u64;
        printer.consume(&samples);
    }
    printer.finish();
    ensure!(
        !printer.fingerprint().is_empty(),
        "Audio is too short to identify"
    );
    let compressed =
        rusty_chromaprint::FingerprintCompressor::from(&config).compress(printer.fingerprint());
    Ok((
        duration.unwrap_or(count / rate as u64 / channels as u64),
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(compressed),
    ))
}
