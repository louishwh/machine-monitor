use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use fw_proto::messages::ServerToAgent;

/// Result of a command sent to an agent.
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub cmd_id: String,
    pub exit: i32,
    pub stdout: String,
    pub stderr: String,
    pub done: bool,
}

/// `(generation, sender)` entry stored per machine connection.
type SenderEntry = (u64, mpsc::Sender<ServerToAgent>);

#[derive(Default)]
pub struct Conns {
    next_gen: AtomicU64,
    senders: Arc<RwLock<HashMap<String, SenderEntry>>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<CommandResult>>>>,
}

// Manual Clone: AtomicU64 isn't Clone, but we share the same Arc-backed state
// via the Arc<RwLock<...>> fields. The AtomicU64 lives only on the canonical
// instance; clones share the senders/pending Arcs and carry a dummy counter.
impl Clone for Conns {
    fn clone(&self) -> Self {
        Self {
            next_gen: AtomicU64::new(0), // not used on clones
            senders: Arc::clone(&self.senders),
            pending: Arc::clone(&self.pending),
        }
    }
}

impl Conns {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a connection for `id`.
    /// Returns `(receiver, generation)`. The caller must pass the returned
    /// generation to `unregister_gen` so a stale disconnect cannot evict a
    /// newer connection that arrived before cleanup runs.
    pub async fn register(&self, id: &str) -> (mpsc::Receiver<ServerToAgent>, u64) {
        let gen = self.next_gen.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(32);
        self.senders.write().await.insert(id.to_string(), (gen, tx));
        (rx, gen)
    }

    /// Remove the connection entry for `id` **only** if its stored generation
    /// matches `gen`. This prevents a stale old-task cleanup from evicting a
    /// newer connection that registered after the old one disconnected.
    pub async fn unregister_gen(&self, id: &str, gen: u64) {
        let mut senders = self.senders.write().await;
        if let Some(&(stored_gen, _)) = senders.get(id) {
            if stored_gen == gen {
                senders.remove(id);
            }
        }
    }

    /// Send a message to the connected agent. Returns Err if no connection exists.
    pub async fn send(&self, id: &str, msg: ServerToAgent) -> anyhow::Result<()> {
        let senders = self.senders.read().await;
        let (_, tx) = senders
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("no connection for {id}"))?;
        tx.send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("send failed: receiver dropped for {id}"))?;
        Ok(())
    }

    /// Register a pending command result waiter. Returns the receiver end.
    pub async fn new_pending(&self, cmd_id: &str) -> oneshot::Receiver<CommandResult> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(cmd_id.to_string(), tx);
        rx
    }

    /// Resolve a pending command result. If no waiter exists, this is a no-op.
    pub async fn resolve(&self, cmd_id: &str, result: CommandResult) {
        if let Some(tx) = self.pending.lock().await.remove(cmd_id) {
            let _ = tx.send(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fw_proto::messages::ServerToAgent;

    fn sample_result() -> CommandResult {
        CommandResult {
            cmd_id: "c-1".into(),
            exit: 0,
            stdout: "{}".into(),
            stderr: String::new(),
            done: true,
        }
    }

    #[tokio::test]
    async fn register_send_unregister() {
        let conns = Conns::new();
        let (mut rx, gen) = conns.register("m-1").await;
        conns.send("m-1", ServerToAgent::Ping).await.unwrap();
        assert!(rx.recv().await.is_some());
        conns.unregister_gen("m-1", gen).await;
        assert!(conns.send("m-1", ServerToAgent::Ping).await.is_err());
    }

    #[tokio::test]
    async fn stale_unregister_does_not_evict_new_connection() {
        let conns = Conns::new();
        // Simulate old connection
        let (_rx_old, gen_old) = conns.register("m-1").await;
        // New connection arrives, overwrites old sender
        let (mut rx_new, _gen_new) = conns.register("m-1").await;
        // Old task's cleanup fires with stale gen — must be a no-op
        conns.unregister_gen("m-1", gen_old).await;
        // New connection must still be reachable
        conns.send("m-1", ServerToAgent::Ping).await.unwrap();
        assert!(rx_new.recv().await.is_some());
    }

    #[tokio::test]
    async fn pending_result_roundtrip() {
        let conns = Conns::new();
        let waiter = conns.new_pending("c-1").await;
        conns.resolve("c-1", sample_result()).await;
        assert_eq!(waiter.await.unwrap().cmd_id, "c-1");
    }
}
