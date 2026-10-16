//! Compatible ndk-context API plus an application context for background services.
//! Based on ndk-context 0.1.1 by The Rust Windowing contributors (MIT OR Apache-2.0).
use std::{ffi::c_void, sync::Mutex};
#[derive(Clone, Copy, Debug)]
pub struct AndroidContext {
    java_vm: *mut c_void,
    context_jobject: *mut c_void,
}
impl AndroidContext {
    pub fn vm(self) -> *mut c_void {
        self.java_vm
    }
    pub fn context(self) -> *mut c_void {
        self.context_jobject
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pointers {
    vm: usize,
    context: usize,
}
#[derive(Default)]
struct State {
    activity: Option<Pointers>,
    application: Option<Pointers>,
}
impl State {
    fn initialize(&mut self, context: Pointers) {
        assert!(
            self.activity.is_none(),
            "android activity context already initialized"
        );
        self.activity = Some(context);
    }
    fn release(&mut self) {
        assert!(
            self.activity.take().is_some(),
            "android activity context was not initialized"
        );
    }
    fn application(&mut self, context: Pointers) {
        if let Some(previous) = self.application {
            assert_eq!(previous.vm, context.vm, "Android VM changed");
        } else {
            self.application = Some(context);
        }
    }
    fn current(&self) -> Pointers {
        self.application
            .or(self.activity)
            .expect("android context was not initialized")
    }
}
static STATE: Mutex<State> = Mutex::new(State {
    activity: None,
    application: None,
});
pub fn android_context() -> AndroidContext {
    let context = STATE
        .lock()
        .expect("Android context lock poisoned")
        .current();
    AndroidContext {
        java_vm: context.vm as *mut c_void,
        context_jobject: context.context as *mut c_void,
    }
}
/// Initialize the activity context, preserving the upstream lifecycle contract.
/// # Safety
/// Both pointers must stay valid until release; initialize once per activity lifetime.
pub unsafe fn initialize_android_context(java_vm: *mut c_void, context_jobject: *mut c_void) {
    STATE
        .lock()
        .expect("Android context lock poisoned")
        .initialize(Pointers {
            vm: java_vm as usize,
            context: context_jobject as usize,
        });
}
/// Release the activity context. A separately registered Application remains usable.
/// # Safety
/// Call only after activity initialization and when its JNI context is no longer in use.
pub unsafe fn release_android_context() {
    STATE
        .lock()
        .expect("Android context lock poisoned")
        .release();
}
/// Register a stable Application context without initializing an Activity.
/// # Safety
/// Both pointers must be valid for the remainder of the process. `context_jobject`
/// must be a retained JNI global reference to the Application, never an Activity.
pub unsafe fn set_application_context(java_vm: *mut c_void, context_jobject: *mut c_void) {
    assert!(!java_vm.is_null() && !context_jobject.is_null());
    STATE
        .lock()
        .expect("Android context lock poisoned")
        .application(Pointers {
            vm: java_vm as usize,
            context: context_jobject as usize,
        });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn application_survives_activity_recreation() {
        let app = Pointers { vm: 1, context: 2 };
        let first = Pointers { vm: 1, context: 3 };
        let mut s = State::default();
        s.application(app);
        assert_eq!(s.current(), app);
        s.initialize(first);
        assert_eq!(s.current(), app);
        s.release();
        assert_eq!(s.current(), app);
        s.initialize(Pointers { vm: 1, context: 4 });
        s.release();
        assert_eq!(s.current(), app);
        s.application(Pointers { vm: 1, context: 5 });
        assert_eq!(s.current(), app);
    }
    #[test]
    fn upstream_activity_only_lifecycle_is_preserved() {
        let mut s = State::default();
        let first = Pointers { vm: 1, context: 3 };
        s.initialize(first);
        assert_eq!(s.current(), first);
        s.release();
        assert!(s.activity.is_none());
    }
}
