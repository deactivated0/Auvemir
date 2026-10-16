//! Playback processing lives on the decoder producer, never on the device callback.
use std::sync::atomic::{AtomicU32, Ordering};
pub const FREQUENCIES: [f32; 10] = [
    31., 62., 125., 250., 500., 1000., 2000., 4000., 8000., 16000.,
];
pub struct Settings {
    pub tempo: AtomicU32,
    pub gains: [AtomicU32; 10],
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            tempo: AtomicU32::new(1f32.to_bits()),
            gains: std::array::from_fn(|_| AtomicU32::new(0f32.to_bits())),
        }
    }
}
impl Settings {
    pub fn tempo(&self) -> f32 {
        f32::from_bits(self.tempo.load(Ordering::Relaxed)).clamp(0.5, 2.)
    }
    pub fn set_tempo(&self, value: f32) {
        self.tempo
            .store(value.clamp(0.5, 2.).to_bits(), Ordering::Relaxed);
    }
    pub fn gains(&self) -> [f32; 10] {
        std::array::from_fn(|i| f32::from_bits(self.gains[i].load(Ordering::Relaxed)))
    }
    pub fn set_gain(&self, index: usize, value: f32) {
        if let Some(g) = self.gains.get(index) {
            g.store(value.clamp(-12., 12.).to_bits(), Ordering::Relaxed);
        }
    }
}
#[derive(Clone, Copy)]
struct Filter {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}
impl Default for Filter {
    fn default() -> Self {
        Self {
            b: [1., 0., 0.],
            a: [0., 0.],
            z: [0., 0.],
        }
    }
}
impl Filter {
    fn configure(&mut self, frequency: f32, rate: f32, gain: f32) {
        let w = 2. * std::f32::consts::PI * frequency.min(rate * 0.45) / rate;
        let a = 10f32.powf(gain / 40.);
        let alpha = w.sin() / (2. * 1.4);
        let denom = 1. + alpha / a;
        self.b = [
            (1. + alpha * a) / denom,
            -2. * w.cos() / denom,
            (1. - alpha * a) / denom,
        ];
        self.a = [-2. * w.cos() / denom, (1. - alpha / a) / denom];
    }
    fn sample(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}
pub struct Processor {
    channels: usize,
    rate: u32,
    filters: Vec<[Filter; 10]>,
    gains: [f32; 10],
    stretch: Option<soundtouch::SoundTouch>,
    tempo: f32,
}
impl Processor {
    pub fn new(channels: usize, rate: u32) -> Self {
        Self {
            channels,
            rate,
            filters: vec![[Filter::default(); 10]; channels],
            gains: [0.; 10],
            stretch: None,
            tempo: 1.,
        }
    }
    pub fn process(&mut self, input: &mut [f32], settings: &Settings) {
        let gains = settings.gains();
        if gains != self.gains {
            for channel in &mut self.filters {
                for (i, filter) in channel.iter_mut().enumerate() {
                    filter.configure(FREQUENCIES[i], self.rate as f32, gains[i]);
                }
            }
            self.gains = gains;
        }
        if gains.iter().any(|g| g.abs() > 0.001) {
            // Headroom matches the largest boost. A final limiter prevents clipping.
            let headroom = 10f32.powf(-gains.iter().copied().fold(0f32, f32::max) / 20.);
            for (i, sample) in input.iter_mut().enumerate() {
                let mut x = *sample * headroom;
                for filter in &mut self.filters[i % self.channels] {
                    x = filter.sample(x);
                }
                *sample = x.clamp(-1., 1.);
            }
        }
        let tempo = settings.tempo();
        if tempo != 1. && self.stretch.is_none() {
            let mut st = soundtouch::SoundTouch::new();
            st.set_channels(self.channels as u32)
                .set_sample_rate(self.rate);
            self.stretch = Some(st);
        }
        if let Some(st) = &mut self.stretch {
            if tempo != self.tempo {
                st.set_tempo(tempo as f64);
                self.tempo = tempo;
            }
            st.put_samples(input, input.len() / self.channels);
        }
    }
    pub fn stretched(&self) -> bool {
        self.stretch.is_some()
    }
    pub fn receive(&mut self, output: &mut [f32]) -> usize {
        self.stretch
            .as_mut()
            .map(|s| s.receive_samples(output, output.len() / self.channels) * self.channels)
            .unwrap_or(0)
    }
    pub fn flush(&mut self) {
        if let Some(st) = &mut self.stretch {
            st.flush();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tempo_changes_duration_without_changing_tone() {
        let settings = Settings::default();
        settings.set_tempo(1.5);
        let mut p = Processor::new(1, 48000);
        let mut output = Vec::new();
        let mut buf = [0.; 8192];
        for i in 0..48 {
            let mut input = (0..1000)
                .map(|n| {
                    ((i * 1000 + n) as f32 * 2. * std::f32::consts::PI * 440. / 48000.).sin() * 0.5
                })
                .collect::<Vec<_>>();
            p.process(&mut input, &settings);
            loop {
                let n = p.receive(&mut buf);
                if n == 0 {
                    break;
                }
                output.extend_from_slice(&buf[..n]);
            }
        }
        p.flush();
        loop {
            let n = p.receive(&mut buf);
            if n == 0 {
                break;
            }
            output.extend_from_slice(&buf[..n]);
        }
        assert!((31000..33000).contains(&output.len()));
        let crossing = output
            .windows(2)
            .filter(|p| p[0] <= 0. && p[1] > 0.)
            .count();
        let hz = crossing as f32 * 48000. / output.len() as f32;
        assert!((hz - 440.).abs() < 6., "estimated pitch {hz}");
    }
    #[test]
    fn eq_has_headroom_and_finite_output() {
        let settings = Settings::default();
        for i in 0..10 {
            settings.set_gain(i, 12.);
        }
        let mut p = Processor::new(2, 48000);
        let mut input = (0..96000)
            .map(|i| (i as f32 * 0.1).sin())
            .collect::<Vec<_>>();
        p.process(&mut input, &settings);
        assert!(input.iter().all(|s| s.is_finite() && s.abs() <= 1.));
    }
}
