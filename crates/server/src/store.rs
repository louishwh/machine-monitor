use sqlx::{Row, SqlitePool};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub id: i64,
    pub machine_id: String,
    pub kind: String,
    pub json: String,
    pub captured_at: String,
}

#[derive(Debug, Serialize)]
pub struct Machine {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub os: String,
    pub agent_version: String,
    pub status: String,
    pub last_seen: Option<String>,
}

pub async fn upsert_machine(
    pool: &SqlitePool, id: &str, name: &str, hostname: &str, os: &str, ver: &str,
) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO machines (id,name,hostname,os,agent_version,last_seen,enrolled_at)
         VALUES (?,?,?,?,?,?,?)
         ON CONFLICT(id) DO UPDATE SET
           name=excluded.name, hostname=excluded.hostname, os=excluded.os,
           agent_version=excluded.agent_version, last_seen=excluded.last_seen",
    )
    .bind(id).bind(name).bind(hostname).bind(os).bind(ver).bind(&now).bind(&now)
    .execute(pool).await?;
    Ok(())
}

pub async fn touch_last_seen(pool: &SqlitePool, id: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE machines SET last_seen=? WHERE id=?")
        .bind(&now).bind(id).execute(pool).await?;
    Ok(())
}

pub async fn is_revoked(pool: &SqlitePool, id: &str) -> anyhow::Result<bool> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM revocations WHERE machine_id=?")
        .bind(id).fetch_one(pool).await?;
    Ok(n > 0)
}

pub async fn list_machines(pool: &SqlitePool) -> anyhow::Result<Vec<Machine>> {
    let rows = sqlx::query(
        "SELECT id,name,hostname,os,agent_version,status,last_seen FROM machines ORDER BY name",
    ).fetch_all(pool).await?;
    Ok(rows.into_iter().map(|r| Machine {
        id: r.get("id"), name: r.get("name"), hostname: r.get("hostname"),
        os: r.get("os"), agent_version: r.get("agent_version"),
        status: r.get("status"), last_seen: r.get("last_seen"),
    }).collect())
}

pub async fn save_snapshot(
    pool: &SqlitePool, machine_id: &str, kind: &str, json: &str,
) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO status_snapshots (machine_id, kind, json, captured_at) VALUES (?,?,?,?)",
    )
    .bind(machine_id).bind(kind).bind(json).bind(&now)
    .execute(pool).await?;
    Ok(())
}

/// Returns (json, captured_at) for the most recent snapshot of given machine + kind.
pub async fn latest_snapshot(
    pool: &SqlitePool, machine_id: &str, kind: &str,
) -> anyhow::Result<Option<(String, String)>> {
    let row = sqlx::query(
        "SELECT json, captured_at FROM status_snapshots
         WHERE machine_id=? AND kind=?
         ORDER BY captured_at DESC LIMIT 1",
    )
    .bind(machine_id).bind(kind)
    .fetch_optional(pool).await?;
    Ok(row.map(|r| (r.get::<String, _>("json"), r.get::<String, _>("captured_at"))))
}

pub async fn list_snapshots(
    pool: &SqlitePool, machine_id: &str, limit: i64,
) -> anyhow::Result<Vec<Snapshot>> {
    let rows = sqlx::query(
        "SELECT id, machine_id, kind, json, captured_at FROM status_snapshots
         WHERE machine_id=?
         ORDER BY captured_at DESC LIMIT ?",
    )
    .bind(machine_id).bind(limit)
    .fetch_all(pool).await?;
    Ok(rows.into_iter().map(|r| Snapshot {
        id: r.get("id"),
        machine_id: r.get("machine_id"),
        kind: r.get("kind"),
        json: r.get("json"),
        captured_at: r.get("captured_at"),
    }).collect())
}

// ── console pairing ──────────────────────────────────────────────────────────

/// Returns the stored base64-encoded console public key, or `None` if not yet paired.
pub async fn get_console_pubkey(pool: &SqlitePool) -> anyhow::Result<Option<String>> {
    let row = sqlx::query("SELECT console_public_key FROM server_config WHERE id=1")
        .fetch_one(pool).await?;
    Ok(row.get::<Option<String>, _>("console_public_key"))
}

/// Store the console public key (base64) and record the pairing timestamp.
pub async fn set_console_pubkey(pool: &SqlitePool, pubkey_b64: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "UPDATE server_config SET console_public_key=?, paired_at=? WHERE id=1",
    )
    .bind(pubkey_b64)
    .bind(&now)
    .execute(pool).await?;
    Ok(())
}

/// Returns `true` if a console public key has been registered (server is paired).
pub async fn is_paired(pool: &SqlitePool) -> anyhow::Result<bool> {
    let key = get_console_pubkey(pool).await?;
    Ok(key.is_some())
}

/// Delete snapshots older than `days` days. Returns count of deleted rows.
pub async fn purge_old_snapshots(pool: &SqlitePool, days: i64) -> anyhow::Result<u64> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
    let res = sqlx::query(
        "DELETE FROM status_snapshots WHERE captured_at < ?",
    )
    .bind(&cutoff)
    .execute(pool).await?;
    Ok(res.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[tokio::test]
    async fn upsert_and_list_machine() {
        let pool = db::init_pool_in_memory().await.unwrap();
        upsert_machine(&pool, "m-1", "web-01", "web-01.local", "ubuntu", "0.1.0")
            .await.unwrap();
        let list = list_machines(&pool).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "web-01");
        // re-upsert updates, not duplicates
        upsert_machine(&pool, "m-1", "web-01b", "h", "ubuntu", "0.1.0").await.unwrap();
        assert_eq!(list_machines(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn pairing_state() {
        let pool = db::init_pool_in_memory().await.unwrap();

        // Initially unpaired
        assert!(!is_paired(&pool).await.unwrap(), "should start unpaired");
        assert!(get_console_pubkey(&pool).await.unwrap().is_none());

        // Set a key
        set_console_pubkey(&pool, "dGVzdGtleQ==").await.unwrap();

        // Now paired
        assert!(is_paired(&pool).await.unwrap(), "should be paired after set");
        let key = get_console_pubkey(&pool).await.unwrap();
        assert_eq!(key.as_deref(), Some("dGVzdGtleQ=="));
    }

    #[tokio::test]
    async fn snapshot_save_latest_purge() {
        let pool = db::init_pool_in_memory().await.unwrap();

        // Save two snapshots for same machine+kind
        save_snapshot(&pool, "m-1", "summary", r#"{"cpu_pct":10.0}"#).await.unwrap();
        // Small delay to ensure ordering by captured_at text sort
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        save_snapshot(&pool, "m-1", "summary", r#"{"cpu_pct":20.0}"#).await.unwrap();

        // latest returns the newest
        let (json, _) = latest_snapshot(&pool, "m-1", "summary").await.unwrap().unwrap();
        assert!(json.contains("20.0"), "expected latest snapshot: {json}");

        // list returns 2
        let snaps = list_snapshots(&pool, "m-1", 50).await.unwrap();
        assert_eq!(snaps.len(), 2);

        // Age one record to 40 days ago via direct UPDATE
        let old_time = (chrono::Utc::now() - chrono::Duration::days(40)).to_rfc3339();
        sqlx::query("UPDATE status_snapshots SET captured_at=? WHERE id=(SELECT MIN(id) FROM status_snapshots)")
            .bind(&old_time)
            .execute(&pool).await.unwrap();

        // purge(30) should delete exactly 1
        let deleted = purge_old_snapshots(&pool, 30).await.unwrap();
        assert_eq!(deleted, 1, "expected 1 deleted, got {deleted}");

        // Only 1 remains
        let snaps = list_snapshots(&pool, "m-1", 50).await.unwrap();
        assert_eq!(snaps.len(), 1);
    }
}
