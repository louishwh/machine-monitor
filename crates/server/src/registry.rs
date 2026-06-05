use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub struct Registry {
    online: Arc<RwLock<HashMap<String, chrono::DateTime<chrono::Utc>>>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }
    pub async fn mark_online(&self, id: &str) {
        self.online
            .write()
            .await
            .insert(id.to_string(), chrono::Utc::now());
    }
    pub async fn mark_offline(&self, id: &str) {
        self.online.write().await.remove(id);
    }
    pub async fn is_online(&self, id: &str) -> bool {
        self.online.read().await.contains_key(id)
    }
    pub async fn online_ids(&self) -> Vec<String> {
        self.online.read().await.keys().cloned().collect()
    }
    pub async fn mark_online_at(&self, id: &str, at: chrono::DateTime<chrono::Utc>) {
        self.online.write().await.insert(id.to_string(), at);
    }
    pub async fn last_seen(&self, id: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        self.online.read().await.get(id).copied()
    }
    pub async fn snapshot(&self) -> Vec<(String, chrono::DateTime<chrono::Utc>)> {
        self.online
            .read()
            .await
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn mark_online_offline() {
        let reg = Registry::new();
        reg.mark_online("m-1").await;
        assert!(reg.is_online("m-1").await);
        reg.mark_offline("m-1").await;
        assert!(!reg.is_online("m-1").await);
    }
}
