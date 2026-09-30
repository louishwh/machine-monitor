use fw_proto::messages::ServerToAgent;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tokio::sync::{mpsc, oneshot, watch, RwLock};

/// Result of a command sent to an agent.
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub cmd_id: String,
    pub exit: i32,
    pub stdout: String,
    pub stderr: String,
    pub done: bool,
}

/// `(generation, sender, disconnect_reason)` entry per machine connection.
type SenderEntry = (
    u64,
    mpsc::Sender<ServerToAgent>,
    watch::Sender<Option<&'static str>>,
);
type PendingEntry = (u64, oneshot::Sender<CommandResult>);
type PendingMap = Arc<StdMutex<HashMap<String, PendingEntry>>>;

pub struct Conns {
    next_gen: Arc<AtomicU64>,
    senders: Arc<RwLock<HashMap<String, SenderEntry>>>,
    pending: PendingMap,
}

/// Removes its pending result slot on timeout, send failure, or task cancellation.
pub struct PendingWaiter {
    cmd_id: String,
    generation: u64,
    receiver: oneshot::Receiver<CommandResult>,
    pending: PendingMap,
}

impl PendingWaiter {
    pub async fn recv(&mut self) -> Result<CommandResult, oneshot::error::RecvError> {
        (&mut self.receiver).await
    }
}

impl Drop for PendingWaiter {
    fn drop(&mut self) {
        let mut pending = self.pending.lock().unwrap();
        if pending
            .get(&self.cmd_id)
            .is_some_and(|(generation, _)| *generation == self.generation)
        {
            pending.remove(&self.cmd_id);
        }
    }
}

impl Default for Conns {
    fn default() -> Self {
        Self::new()
    }
}

// All fields are Arc-backed, so Clone is a shallow reference copy and all
// clones share the same generation counter, senders map, and pending map.
impl Clone for Conns {
    fn clone(&self) -> Self {
        Self {
            next_gen: Arc::clone(&self.next_gen),
            senders: Arc::clone(&self.senders),
            pending: Arc::clone(&self.pending),
        }
    }
}

impl Conns {
    pub fn new() -> Self {
        Self {
            next_gen: Arc::new(AtomicU64::new(1)),
            senders: Arc::new(RwLock::new(HashMap::new())),
            pending: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    /// Register a connection for `id`.
    /// Returns `(receiver, disconnect_reason, generation)`.
    /// - The caller drives `rx` to receive outbound `ServerToAgent` messages.
    /// - The caller awaits `disconnect_reason.changed()` for a forced kick.
    /// - The caller must pass `generation` to `unregister_gen` so a stale
    ///   disconnect cannot evict a newer connection registered afterwards.
    pub async fn register(
        &self,
        id: &str,
    ) -> (
        mpsc::Receiver<ServerToAgent>,
        watch::Receiver<Option<&'static str>>,
        u64,
    ) {
        let gen = self.next_gen.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(32);
        let (kill_tx, kill_rx) = watch::channel(None);
        let old = self
            .senders
            .write()
            .await
            .insert(id.to_string(), (gen, tx, kill_tx));
        if let Some((_, _, old_kill)) = old {
            let _ = old_kill.send(Some("连接被新会话替换"));
        }
        (rx, kill_rx, gen)
    }

    /// Force-disconnect the agent identified by `id`.
    /// Sends a disconnect reason and removes the sender so subsequent `send()` calls
    /// return an error.
    pub async fn kick(&self, id: &str) {
        let entry = self.senders.write().await.remove(id);
        if let Some((_, _, kill)) = entry {
            let _ = kill.send(Some("已吊销"));
        }
    }

    /// Remove the connection entry for `id` **only** if its stored generation
    /// matches `gen`. This prevents a stale old-task cleanup from evicting a
    /// newer connection that registered after the old one disconnected.
    /// Returns true only when this generation was the active connection.
    pub async fn unregister_gen(&self, id: &str, gen: u64) -> bool {
        let mut senders = self.senders.write().await;
        if let Some(&(stored_gen, _, _)) = senders.get(id) {
            if stored_gen == gen {
                senders.remove(id);
                return true;
            }
        }
        false
    }

    pub async fn is_current(&self, id: &str, gen: u64) -> bool {
        self.senders
            .read()
            .await
            .get(id)
            .is_some_and(|(stored_gen, _, _)| *stored_gen == gen)
    }

    /// Send a message to the connected agent. Returns Err if no connection exists.
    pub async fn send(&self, id: &str, msg: ServerToAgent) -> anyhow::Result<()> {
        let senders = self.senders.read().await;
        let (_, tx, _) = senders
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("no connection for {id}"))?;
        tx.send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("send failed: receiver dropped for {id}"))?;
        Ok(())
    }

    /// Register a pending command result waiter. Returns the receiver end.
    pub async fn new_pending(&self, cmd_id: &str) -> PendingWaiter {
        let (tx, rx) = oneshot::channel();
        let generation = self.next_gen.fetch_add(1, Ordering::Relaxed);
        self.pending
            .lock()
            .unwrap()
            .insert(cmd_id.to_string(), (generation, tx));
        PendingWaiter {
            cmd_id: cmd_id.to_string(),
            generation,
            receiver: rx,
            pending: Arc::clone(&self.pending),
        }
    }

    /// Resolve a pending command result. If no waiter exists, this is a no-op.
    pub async fn resolve(&self, cmd_id: &str, result: CommandResult) {
        let sender = self.pending.lock().unwrap().remove(cmd_id);
        if let Some((_, tx)) = sender {
            let _ = tx.send(result);
        }
    }

    #[cfg(test)]
    pub(crate) fn pending_len(&self) -> usize {
        self.pending.lock().unwrap().len()
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
        let (mut rx, _kill, gen) = conns.register("m-1").await;
        conns.send("m-1", ServerToAgent::Ping).await.unwrap();
        assert!(rx.recv().await.is_some());
        conns.unregister_gen("m-1", gen).await;
        assert!(conns.send("m-1", ServerToAgent::Ping).await.is_err());
    }

    #[tokio::test]
    async fn stale_unregister_does_not_evict_new_connection() {
        let conns = Conns::new();
        // Simulate old connection
        let (_rx_old, mut kill_old, gen_old) = conns.register("m-1").await;
        // New connection arrives, overwrites old sender
        let (mut rx_new, _kill_new, _gen_new) = conns.register("m-1").await;
        kill_old.changed().await.unwrap();
        assert_eq!(*kill_old.borrow(), Some("连接被新会话替换"));
        // Old task's cleanup fires with stale gen — must be a no-op
        assert!(!conns.unregister_gen("m-1", gen_old).await);
        assert!(!conns.is_current("m-1", gen_old).await);
        // New connection must still be reachable
        conns.send("m-1", ServerToAgent::Ping).await.unwrap();
        assert!(rx_new.recv().await.is_some());
    }

    #[tokio::test]
    async fn cloned_conns_share_generation_counter() {
        let a = Conns::new();
        let b = a.clone();
        let (_rx1, _k1, g1) = a.register("m-1").await;
        let (_rx2, _k2, g2) = b.register("m-2").await;
        assert_ne!(g1, g2); // distinct generations across clones
    }

    #[tokio::test]
    async fn kick_signals() {
        let conns = Conns::new();
        let (_rx, mut kill, _gen) = conns.register("m-1").await;

        // Kick the connection — must wake the waiting task within 500 ms.
        conns.kick("m-1").await;
        tokio::time::timeout(tokio::time::Duration::from_millis(500), kill.changed())
            .await
            .expect("kick did not signal within timeout")
            .unwrap();
        assert_eq!(*kill.borrow(), Some("已吊销"));

        // Sender must be gone after kick.
        assert!(conns.send("m-1", ServerToAgent::Ping).await.is_err());
    }

    #[tokio::test]
    async fn pending_result_roundtrip() {
        let conns = Conns::new();
        let mut waiter = conns.new_pending("c-1").await;
        conns.resolve("c-1", sample_result()).await;
        assert_eq!(waiter.recv().await.unwrap().cmd_id, "c-1");
        assert_eq!(conns.pending_len(), 0);
    }

    #[tokio::test]
    async fn dropping_waiter_cleans_only_its_own_generation() {
        let conns = Conns::new();
        let old = conns.new_pending("c-1").await;
        let newer = conns.new_pending("c-1").await;
        drop(old);
        assert_eq!(conns.pending_len(), 1);
        drop(newer);
        assert_eq!(conns.pending_len(), 0);
    }
}
