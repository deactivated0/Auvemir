//! Native secure storage; no plaintext fallback or credential-bearing database URLs.
use anyhow::{Context, Result};
pub fn save(id: &str, secret: &str) -> Result<()> {
    keyring::Entry::new("Auvemir", id)
        .context("Operating-system secure storage is unavailable")?
        .set_password(secret)
        .context("Could not save the credential in secure storage")?;
    Ok(())
}
pub fn load(id: &str) -> Result<String> {
    keyring::Entry::new("Auvemir", id)
        .context("Operating-system secure storage is unavailable")?
        .get_password()
        .context("Unlock secure storage or reconnect this service")
}
pub fn remove(id: &str) -> Result<()> {
    let entry = keyring::Entry::new("Auvemir", id)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
