use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS machines (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  hostname TEXT NOT NULL DEFAULT '',
  os TEXT NOT NULL DEFAULT '',
  agent_version TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active',
  shell_enabled INTEGER NOT NULL DEFAULT 0,
  last_seen TEXT,
  enrolled_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS revocations (
  machine_id TEXT PRIMARY KEY, revoked_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS status_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  machine_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  json TEXT NOT NULL,
  captured_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_snap_machine ON status_snapshots(machine_id, captured_at);
"#;

pub async fn init_pool(db_path: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{db_path}"))?
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new().max_connections(4).connect_with(opts).await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

pub async fn init_pool_in_memory() -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")?;
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    for stmt in SCHEMA.split(';') {
        let s = stmt.trim();
        if !s.is_empty() { sqlx::query(s).execute(pool).await?; }
    }
    Ok(())
}
