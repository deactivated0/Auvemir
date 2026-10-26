//! Read platform accessibility preferences once, never from an animation frame.
use std::sync::OnceLock;

pub fn system_reduced() -> bool {
    *OnceLock::get_or_init(&PREFERENCE, || {
        std::env::var("AUVEMIR_REDUCE_MOTION").is_ok_and(|v| v == "1" || v == "true")
            || platform_reduced()
    })
}
static PREFERENCE: OnceLock<bool> = OnceLock::new();

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn query(program: &str, args: &[&str]) -> Option<String> {
    use std::{
        io::Read,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_millis(150);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut output = String::new();
                child
                    .stdout
                    .take()?
                    .take(256)
                    .read_to_string(&mut output)
                    .ok()?;
                return Some(output.trim().into());
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}
#[cfg(target_os = "linux")]
fn platform_reduced() -> bool {
    if query(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "enable-animations"],
    )
    .as_deref()
        == Some("false")
    {
        return true;
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join(".config")));
    config
        .and_then(|p| std::fs::read_to_string(p.join("kdeglobals")).ok())
        .is_some_and(|s| {
            s.lines().any(|line| {
                line.trim()
                    .strip_prefix("AnimationDurationFactor=")
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    == Some(0.)
            })
        })
}
#[cfg(target_os = "macos")]
fn platform_reduced() -> bool {
    query(
        "defaults",
        &["read", "com.apple.universalaccess", "reduceMotion"],
    )
    .as_deref()
        == Some("1")
}
#[cfg(target_os = "windows")]
fn platform_reduced() -> bool {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SystemParametersInfoW(
            action: u32,
            parameter: u32,
            value: *mut std::ffi::c_void,
            flags: u32,
        ) -> i32;
    }
    let mut enabled: i32 = 1;
    unsafe {
        SystemParametersInfoW(0x1042, 0, (&mut enabled as *mut i32).cast(), 0) != 0 && enabled == 0
    }
}
#[cfg(target_os = "android")]
fn platform_reduced() -> bool {
    crate::mobile_android::reduced_motion().unwrap_or(false)
}
#[cfg(target_os = "ios")]
fn platform_reduced() -> bool {
    crate::mobile_ios::reduced_motion()
}
#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "windows",
    target_os = "android",
    target_os = "ios"
)))]
fn platform_reduced() -> bool {
    false
}
