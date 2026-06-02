//! Simple keyring-backed settings for the console.
//! Each setting is stored as a string under service `com.fleetwatch.console`.

use keyring::Entry;

use crate::error::{AppError, AppResult};

const SERVICE: &str = "com.fleetwatch.console";
const SERVER_URL_USER: &str = "server_url";

fn entry(user: &str) -> AppResult<Entry> {
    Ok(Entry::new(SERVICE, user)?)
}

pub fn get_server_url() -> AppResult<Option<String>> {
    match entry(SERVER_URL_USER)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    }
}

pub fn set_server_url(url: &str) -> AppResult<()> {
    entry(SERVER_URL_USER)?.set_password(url)?;
    Ok(())
}

pub fn require_server_url() -> AppResult<String> {
    get_server_url()?.ok_or(AppError::NoServerUrl)
}

// ──────────────────────────── Tauri commands ────────────────────────────

#[tauri::command]
pub fn get_server_url_cmd() -> Result<Option<String>, crate::error::AppError> {
    get_server_url()
}

#[tauri::command]
pub fn set_server_url_cmd(url: String) -> Result<(), crate::error::AppError> {
    set_server_url(&url)
}
