//! Remember which parts of the shared mixer a transport pause interrupted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Targets {
    pub primary: bool,
    pub layers: bool,
}

#[derive(Default)]
pub struct Transport {
    pub interrupted: bool,
    pub layers_paused: bool,
    paused: Option<Targets>,
}

impl Transport {
    pub fn pause(&mut self, targets: Targets) {
        self.layers_paused = true;
        // Repeated pause/loss callbacks must preserve the original active set.
        if targets.primary || targets.layers {
            self.paused = Some(targets);
        }
    }
    pub fn play(&mut self, fallback: Targets) -> Targets {
        let targets = self.paused.take().unwrap_or(fallback);
        self.layers_paused = !targets.layers;
        targets
    }
    pub fn focus_gain(&mut self) -> Option<Targets> {
        if !self.interrupted {
            return None;
        }
        self.interrupted = false;
        let targets = self.paused.take();
        self.layers_paused = !targets.is_some_and(|targets| targets.layers);
        targets
    }
    pub fn pause_layers(&mut self) {
        self.layers_paused = true;
        if let Some(targets) = &mut self.paused {
            targets.layers = false;
            if !targets.primary {
                self.paused = None;
            }
        }
    }
    pub fn stop(&mut self) {
        self.interrupted = false;
        self.layers_paused = false;
        self.paused = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_restores_only_the_audio_that_was_active() {
        for primary in [false, true] {
            for layers in [false, true] {
                let mut transport = Transport::default();
                let targets = Targets { primary, layers };
                transport.interrupted = true;
                transport.pause(targets);
                transport.pause(Targets::default());
                assert_eq!(
                    transport.focus_gain(),
                    (primary || layers).then_some(targets)
                );
                assert_eq!(transport.focus_gain(), None);
            }
        }
    }

    #[test]
    fn user_pause_cancels_focus_resume_but_preserves_explicit_resume() {
        let mut transport = Transport::default();
        let ambient = Targets {
            primary: false,
            layers: true,
        };
        transport.interrupted = true;
        transport.pause(ambient);
        transport.interrupted = false;
        transport.pause(Targets::default());
        assert_eq!(transport.focus_gain(), None);
        assert_eq!(
            transport.play(Targets {
                primary: true,
                layers: false
            }),
            ambient
        );
        transport.stop();
        assert_eq!(transport.focus_gain(), None);
        assert_eq!(transport.play(Targets::default()), Targets::default());
    }

    #[test]
    fn pausing_ambient_during_an_interruption_preserves_primary_resume() {
        let mut transport = Transport {
            interrupted: true,
            ..Default::default()
        };
        transport.pause(Targets {
            primary: true,
            layers: true,
        });
        transport.pause_layers();
        assert!(transport.layers_paused);
        assert_eq!(
            transport.focus_gain(),
            Some(Targets {
                primary: true,
                layers: false
            })
        );
        assert!(transport.layers_paused);
    }
}
