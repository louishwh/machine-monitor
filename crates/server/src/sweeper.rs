use crate::{registry::Registry, store, AppState};

pub async fn sweep_once(reg: &Registry, timeout_secs: i64) {
    let now = chrono::Utc::now();
    for (id, seen) in reg.snapshot().await {
        if (now - seen).num_seconds() > timeout_secs {
            reg.mark_offline(&id).await;
        }
    }
}

pub fn spawn(state: AppState) {
    // Offline-scan loop: runs every 15 seconds.
    let reg = state.registry.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            tick.tick().await;
            sweep_once(&reg, 45).await;
        }
    });

    // Daily snapshot purge loop: runs once every 24 hours.
    let pool = state.pool.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(24 * 3600));
        // Consume the first immediate tick so we don't purge on startup.
        tick.tick().await;
        loop {
            tick.tick().await;
            match store::purge_old_snapshots(&pool, 30).await {
                Ok(n) => tracing::info!(deleted = n, "daily snapshot purge complete"),
                Err(e) => tracing::warn!(err = %e, "daily snapshot purge failed"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    #[tokio::test]
    async fn stale_entries_go_offline() {
        let reg = Registry::new();
        reg.mark_online_at("m-1", chrono::Utc::now() - chrono::Duration::seconds(90)).await;
        reg.mark_online("m-2").await;
        sweep_once(&reg, 45).await;
        assert!(!reg.is_online("m-1").await);
        assert!(reg.is_online("m-2").await);
    }
}
