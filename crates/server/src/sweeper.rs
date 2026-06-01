use crate::{registry::Registry, AppState};

pub async fn sweep_once(reg: &Registry, timeout_secs: i64) {
    let now = chrono::Utc::now();
    for (id, seen) in reg.snapshot().await {
        if (now - seen).num_seconds() > timeout_secs {
            reg.mark_offline(&id).await;
        }
    }
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            tick.tick().await;
            sweep_once(&state.registry, 45).await;
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
