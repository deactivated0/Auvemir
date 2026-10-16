#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[path = "credentials_desktop.rs"]
mod desktop;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use desktop::*;
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn save(id: &str, secret: &str) -> anyhow::Result<()> {
    crate::platform::credential("save", id, secret).map(|_| ())
}
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn load(id: &str) -> anyhow::Result<String> {
    crate::platform::credential("load", id, "")
}
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn remove(id: &str) -> anyhow::Result<()> {
    crate::platform::credential("remove", id, "").map(|_| ())
}
