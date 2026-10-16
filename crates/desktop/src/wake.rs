use std::time::{Duration, Instant};

pub struct Schedule {
    pub audio_ui: bool,
    pub casting: bool,
    pub sleep: Option<Instant>,
    pub scrobble: Option<Instant>,
}

impl Schedule {
    pub fn delay(&self, now: Instant) -> Option<Duration> {
        [
            self.audio_ui.then_some(Duration::from_millis(250)),
            self.casting.then_some(Duration::from_secs(1)),
            self.sleep.map(|s| s.saturating_duration_since(now)),
            self.scrobble.map(|s| {
                s.saturating_duration_since(now)
                    .clamp(Duration::from_millis(250), Duration::from_secs(30))
            }),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn idle_waits_for_commands_or_the_actual_deadline() {
        let now = Instant::now();
        let mut s = Schedule {
            audio_ui: false,
            casting: false,
            sleep: None,
            scrobble: None,
        };
        assert_eq!(s.delay(now), None);
        s.sleep = Some(now + Duration::from_secs(3600));
        assert_eq!(s.delay(now), Some(Duration::from_secs(3600)));
        s.sleep = Some(now);
        assert_eq!(s.delay(now), Some(Duration::ZERO));
    }
    #[test]
    fn active_cadence_and_earlier_deadlines_are_preserved() {
        let now = Instant::now();
        let mut s = Schedule {
            audio_ui: true,
            casting: true,
            sleep: None,
            scrobble: None,
        };
        assert_eq!(s.delay(now), Some(Duration::from_millis(250)));
        s.audio_ui = false;
        assert_eq!(s.delay(now), Some(Duration::from_secs(1)));
        s.sleep = Some(now + Duration::from_millis(100));
        assert_eq!(s.delay(now), Some(Duration::from_millis(100)));
        s.sleep = None;
        s.casting = false;
        s.scrobble = Some(now + Duration::from_secs(60));
        assert_eq!(s.delay(now), Some(Duration::from_secs(30)));
    }
}
