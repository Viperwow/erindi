//! The local model's API key, kept in the OS credential store instead of `settings.json`.

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new("com.viperwow.erindi", "api-key").map_err(|e| e.to_string())
}

pub fn get() -> Option<String> {
    entry().ok()?.get_password().ok()
}

pub fn set(key: &str) -> Result<(), String> {
    entry()?.set_password(key).map_err(|e| e.to_string())
}

pub fn clear() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cargo test -p erindi-desktop -- --ignored the_key_round_trips`; CI may have no store.
    #[test]
    #[ignore = "writes to the OS credential store"]
    fn the_key_round_trips_through_the_credential_store() {
        set("k").unwrap();
        assert_eq!(get().as_deref(), Some("k"));
        clear().unwrap();
        assert_eq!(get(), None);
        clear().unwrap();
    }
}
