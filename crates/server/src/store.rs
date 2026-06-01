use sqlx::{Row, SqlitePool};
use serde::Serialize;

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
}
