//! Replaceable UI attachment. The audio worker can outlive Android activities.
use crate::app::MainWindow;
#[cfg(not(any(target_os = "android", test)))]
#[derive(Clone)]
pub struct Target(slint::Weak<MainWindow>);
#[cfg(not(any(target_os = "android", test)))]
impl Target {
    pub fn new(window: slint::Weak<MainWindow>) -> Self {
        Self(window)
    }
    pub fn attached(&self) -> bool {
        true
    }
    pub fn upgrade_in_event_loop(
        &self,
        f: impl FnOnce(MainWindow) + Send + 'static,
    ) -> Result<(), slint::EventLoopError> {
        self.0.upgrade_in_event_loop(f)
    }
}
#[cfg(any(target_os = "android", test))]
mod replaceable {
    use super::MainWindow;
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct Slot {
        generation: u64,
        window: Option<slint::Weak<MainWindow>>,
    }
    #[derive(Clone, Default)]
    pub struct Target(Arc<Mutex<Slot>>);
    pub struct Attachment {
        target: Target,
        generation: u64,
    }
    impl Drop for Attachment {
        fn drop(&mut self) {
            if let Ok(mut slot) = self.target.0.lock()
                && slot.generation == self.generation
            {
                slot.window = None;
            }
        }
    }
    impl Target {
        #[cfg(test)]
        pub fn new(window: slint::Weak<MainWindow>) -> Self {
            Self(Arc::new(Mutex::new(Slot {
                generation: 0,
                window: Some(window),
            })))
        }
        pub fn attach(&self, window: slint::Weak<MainWindow>) -> Attachment {
            let mut slot = self.0.lock().expect("UI attachment lock poisoned");
            slot.generation = slot.generation.wrapping_add(1);
            slot.window = Some(window);
            Attachment {
                target: self.clone(),
                generation: slot.generation,
            }
        }
        pub fn attached(&self) -> bool {
            self.0.lock().is_ok_and(|slot| slot.window.is_some())
        }
        pub fn upgrade_in_event_loop(
            &self,
            f: impl FnOnce(MainWindow) + Send + 'static,
        ) -> Result<(), slint::EventLoopError> {
            let window = self.0.lock().ok().and_then(|slot| slot.window.clone());
            let Some(window) = window else {
                return Ok(());
            };
            let result = window.upgrade_in_event_loop(f);
            #[cfg(target_os = "android")]
            if result.is_err() {
                return Ok(());
            } // Activity teardown can race a queued UI update.
            result
        }
    }
}
#[cfg(any(target_os = "android", test))]
pub use replaceable::Target;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detach_from_old_activity_does_not_detach_its_replacement() {
        let target = Target::default();
        let first = target.attach(slint::Weak::default());
        let second = target.attach(slint::Weak::default());
        drop(first);
        assert!(target.attached());
        drop(second);
        assert!(!target.attached());
    }
    #[test]
    fn headless_updates_do_not_require_an_event_loop() {
        Target::default()
            .upgrade_in_event_loop(|_| panic!("No UI is attached"))
            .unwrap();
    }
}
