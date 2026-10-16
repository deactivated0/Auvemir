use anyhow::{Context, Result, ensure};
use std::{
    ffi::{CStr, CString, c_char},
    path::PathBuf,
};
unsafe extern "C" {
    fn auvemir_data_path() -> *mut c_char;
    fn auvemir_pick(page: i32, folder: i32, action: *const c_char);
    fn auvemir_microphone() -> i32;
    fn auvemir_activate(recording: i32) -> i32;
    fn auvemir_recording(active: i32);
    fn auvemir_now_playing(json: *const c_char);
    fn auvemir_credential(
        action: *const c_char,
        id: *const c_char,
        secret: *const c_char,
    ) -> *mut c_char;
    fn auvemir_shutdown();
    fn auvemir_user_transport();
    fn auvemir_routes();
    fn auvemir_export(path: *const c_char, name: *const c_char);
}
fn returned(ptr: *mut c_char) -> Result<String> {
    ensure!(!ptr.is_null(), "Native operation failed");
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    unsafe { libc::free(ptr.cast()) };
    Ok(text)
}
#[unsafe(no_mangle)]
pub extern "C" fn auvemir_ios_run() -> i32 {
    match crate::run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Auvemir: {e:#}");
            1
        }
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn auvemir_ios_command(action: *const c_char, value: *const c_char, n: i64) {
    if action.is_null() || value.is_null() {
        return;
    }
    let action = unsafe { CStr::from_ptr(action) };
    let value = unsafe { CStr::from_ptr(value) };
    if action.to_bytes().len() > 128 || value.to_bytes().len() > 1024 * 1024 {
        return;
    }
    if let (Ok(a), Ok(v)) = (action.to_str(), value.to_str()) {
        crate::platform::command(a.into(), v.into(), n);
    }
}
pub fn data_dir() -> Result<PathBuf> {
    Ok(PathBuf::from(returned(unsafe { auvemir_data_path() })?))
}
pub fn pick(page: i32, folder: bool, action: &str) -> Result<()> {
    let action = CString::new(action)?;
    unsafe { auvemir_pick(page, folder as i32, action.as_ptr()) };
    Ok(())
}
pub fn microphone_ready() -> Result<bool> {
    Ok(unsafe { auvemir_microphone() } != 0)
}
pub fn activate_audio(recording: bool) -> Result<()> {
    ensure!(
        unsafe { auvemir_activate(recording as i32) } != 0,
        "Could not activate the audio session"
    );
    Ok(())
}
pub fn recording(state: i32) {
    unsafe { auvemir_recording(state) }
}
pub fn now_playing(
    media: Option<&auvemir_core::model::Media>,
    playing: bool,
    position: u64,
    volume: f32,
    speed: f32,
    can_seek: bool,
) {
    let json = serde_json::json!({"id":media.map(|m|m.id.as_str()).unwrap_or(""),"title":media.map(|m|m.title.as_str()).unwrap_or(""),"artist":media.map(|m|m.artist.as_str()).unwrap_or(""),"duration":media.map(|m|m.duration_ms).unwrap_or(0),"position":position,"playing":playing,"volume":volume,"speed":speed,"canSeek":can_seek});
    if let Ok(json) = CString::new(json.to_string()) {
        unsafe { auvemir_now_playing(json.as_ptr()) };
    }
}
pub fn credential(action: &str, id: &str, secret: &str) -> Result<String> {
    ensure!(
        id.len() <= 1024 && secret.len() <= 65536,
        "Credential is too large"
    );
    let a = CString::new(action)?;
    let i = CString::new(id)?;
    let s = CString::new(secret)?;
    returned(unsafe { auvemir_credential(a.as_ptr(), i.as_ptr(), s.as_ptr()) })
        .context("Unlock Keychain or reconnect this service")
}
pub fn shutdown() {
    unsafe { auvemir_shutdown() };
}
pub fn routes() {
    unsafe { auvemir_routes() };
}

pub fn export_document(path: &str, name: &str) -> Result<()> {
    let path = CString::new(path)?;
    let name = CString::new(name)?;
    unsafe { auvemir_export(path.as_ptr(), name.as_ptr()) };
    Ok(())
}

pub fn user_transport() {
    unsafe { auvemir_user_transport() };
}
