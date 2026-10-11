//! Tokens live in the macOS Keychain. SQLite only stores that an account exists.

use keyring::Entry;

const SERVICE: &str = "com.streakorb.app";

fn entry(account_id: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account_id).map_err(|err| format!("Keychain error: {err}"))
}

pub fn set_token(account_id: &str, token: &str) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return delete_token(account_id);
    }
    entry(account_id)?
        .set_password(token)
        .map_err(|err| format!("Couldn't save the token to the Keychain: {err}"))
}

pub fn get_token(account_id: &str) -> Result<Option<String>, String> {
    match entry(account_id)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(format!("Couldn't read the token from the Keychain: {err}")),
    }
}

pub fn delete_token(account_id: &str) -> Result<(), String> {
    match entry(account_id)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(format!("Couldn't remove the token from the Keychain: {err}")),
    }
}
