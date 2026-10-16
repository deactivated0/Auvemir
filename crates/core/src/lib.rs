//! Shared application logic. No GUI, device, account, or network is initialized on import.
pub mod ambient;
pub mod cast;
pub mod cast_proxy;
pub mod database;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod discord;
pub mod downloads;
pub mod library;
pub mod listen;
pub mod lyrics;
pub mod metadata;
pub mod model;
pub mod network;
pub mod providers;
pub mod queue;
pub mod radio;
pub mod scrobble;
pub mod servers;
pub mod sync;
