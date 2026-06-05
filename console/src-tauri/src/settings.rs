//! Simple keyring-backed settings for the console.
//! Each setting is stored as a string under service `com.fleetwatch.console`.

use keyring::Entry;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

const SERVICE: &str = "com.fleetwatch.console";
const SERVER_URL_USER: &str = "server_url";
const SERVER_CA_USER: &str = "server_ca";
const SERVERS_USER: &str = "servers";
const ACTIVE_SERVER_USER: &str = "active_server";

fn entry(user: &str) -> AppResult<Entry> {
    Ok(Entry::new(SERVICE, user)?)
}

// ─────────────────────────── ServerProfile ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    pub url: String,
    pub ca: Option<String>,
}

// ─────────────────────────── multi-server helpers ────────────────────────────

/// Read the full server list from keychain. Returns empty vec if not set.
pub fn list_servers() -> AppResult<Vec<ServerProfile>> {
    match entry(SERVERS_USER)?.get_password() {
        Ok(json) => serde_json::from_str(&json).map_err(AppError::Json),
        Err(keyring::Error::NoEntry) => Ok(Vec::new()),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    }
}

/// Persist the full server list to keychain.
pub fn save_servers(profiles: &[ServerProfile]) -> AppResult<()> {
    let json = serde_json::to_string(profiles)?;
    entry(SERVERS_USER)?.set_password(&json)?;
    Ok(())
}

/// Return the active server profile ID (if any).
pub fn get_active_id() -> AppResult<Option<String>> {
    match entry(ACTIVE_SERVER_USER)?.get_password() {
        Ok(id) => Ok(Some(id)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    }
}

/// Persist the active server profile ID.
pub fn set_active_id(id: &str) -> AppResult<()> {
    entry(ACTIVE_SERVER_USER)?.set_password(id)?;
    Ok(())
}

/// Return the full `ServerProfile` for the currently active server.
pub fn get_active_server() -> AppResult<Option<ServerProfile>> {
    let id = match get_active_id()? {
        Some(id) => id,
        None => return Ok(None),
    };
    let servers = list_servers()?;
    Ok(servers.into_iter().find(|s| s.id == id))
}

/// Require an active server profile; return a clear error if none is configured.
pub fn require_active_server() -> AppResult<ServerProfile> {
    get_active_server()?.ok_or(AppError::NoServerUrl)
}

// ─────────────────────────── migration ──────────────────────────────────────

/// One-shot migration: if the new `servers` list is empty/absent but the legacy
/// `server_url` key exists, create a single profile named "默认" from the
/// legacy values and set it active.  Safe to call repeatedly (idempotent).
pub fn ensure_migrated() -> AppResult<()> {
    let existing = list_servers()?;
    if !existing.is_empty() {
        return Ok(());
    }

    // Check for legacy server_url
    let legacy_url = match entry(SERVER_URL_USER)?.get_password() {
        Ok(v) => v,
        Err(keyring::Error::NoEntry) => return Ok(()),
        Err(e) => return Err(AppError::Keyring(e.to_string())),
    };

    let legacy_ca = match entry(SERVER_CA_USER)?.get_password() {
        Ok(v) => Some(v),
        Err(keyring::Error::NoEntry) => None,
        Err(e) => return Err(AppError::Keyring(e.to_string())),
    };

    let id = Uuid::new_v4().to_string();
    let profile = ServerProfile {
        id: id.clone(),
        name: "默认".to_string(),
        url: legacy_url,
        ca: legacy_ca,
    };

    save_servers(&[profile])?;
    set_active_id(&id)?;
    Ok(())
}

// ─────────────────────────── legacy helpers (kept for compat) ────────────────

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
    // Try active profile first (post-migration), fall back to legacy key.
    if let Some(profile) = get_active_server()? {
        return Ok(profile.url);
    }
    get_server_url()?.ok_or(AppError::NoServerUrl)
}

pub fn get_server_ca() -> AppResult<Option<String>> {
    // Try active profile first.
    if let Some(profile) = get_active_server()? {
        return Ok(profile.ca);
    }
    match entry(SERVER_CA_USER)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Keyring(e.to_string())),
    }
}

pub fn set_server_ca(pem: &str) -> AppResult<()> {
    entry(SERVER_CA_USER)?.set_password(pem)?;
    Ok(())
}

// ─────────────────────────── Tauri commands (legacy) ────────────────────────

#[tauri::command]
pub fn get_server_url_cmd() -> Result<Option<String>, crate::error::AppError> {
    get_server_url()
}

#[tauri::command]
pub fn set_server_url_cmd(url: String) -> Result<(), crate::error::AppError> {
    set_server_url(&url)
}

#[tauri::command]
pub fn get_server_ca_cmd() -> Result<Option<String>, crate::error::AppError> {
    get_server_ca()
}

#[tauri::command]
pub fn set_server_ca_cmd(pem: String) -> Result<(), crate::error::AppError> {
    set_server_ca(&pem)
}

// ─────────────────────────── Tauri commands (multi-server) ──────────────────

#[tauri::command]
pub fn list_servers_cmd() -> Result<Vec<ServerProfile>, crate::error::AppError> {
    ensure_migrated()?;
    list_servers()
}

#[tauri::command]
pub fn add_server_cmd(
    name: String,
    url: String,
    ca: Option<String>,
) -> Result<ServerProfile, crate::error::AppError> {
    ensure_migrated()?;
    let mut servers = list_servers()?;
    let id = Uuid::new_v4().to_string();
    let profile = ServerProfile {
        id: id.clone(),
        name,
        url,
        ca,
    };
    let is_first = servers.is_empty();
    servers.push(profile.clone());
    save_servers(&servers)?;
    if is_first {
        set_active_id(&id)?;
    }
    Ok(profile)
}

#[tauri::command]
pub fn update_server_cmd(
    id: String,
    name: String,
    url: String,
    ca: Option<String>,
) -> Result<ServerProfile, crate::error::AppError> {
    ensure_migrated()?;
    let mut servers = list_servers()?;
    let profile = servers
        .iter_mut()
        .find(|s| s.id == id)
        .ok_or_else(|| AppError::Keyring(format!("server profile {id} not found")))?;
    profile.name = name;
    profile.url = url;
    profile.ca = ca;
    let updated = profile.clone();
    save_servers(&servers)?;
    Ok(updated)
}

#[tauri::command]
pub fn remove_server_cmd(id: String) -> Result<(), crate::error::AppError> {
    ensure_migrated()?;
    let mut servers = list_servers()?;
    servers.retain(|s| s.id != id);
    save_servers(&servers)?;

    // If the removed server was active, switch to the first remaining or clear.
    if let Some(active_id) = get_active_id()? {
        if active_id == id {
            if let Some(first) = servers.first() {
                set_active_id(&first.id)?;
            } else {
                // Clear active — no profiles left.
                match entry(ACTIVE_SERVER_USER)?.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => {}
                    Err(e) => return Err(AppError::Keyring(e.to_string())),
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn set_active_server_cmd(id: String) -> Result<(), crate::error::AppError> {
    ensure_migrated()?;
    set_active_id(&id)
}

#[tauri::command]
pub fn get_active_server_cmd() -> Result<Option<ServerProfile>, crate::error::AppError> {
    ensure_migrated()?;
    get_active_server()
}
