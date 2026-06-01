use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use fw_proto::messages::{ServerToAgent, AgentToServer};

/// Result of a command sent to an agent.
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub cmd_id: String,
    pub exit: i32,
    pub stdout: String,
    pub stderr: String,
    pub done: bool,
}

impl From<AgentToServer> for CommandResult {
    fn from(msg: AgentToServer) -> Self {
        match msg {
            AgentToServer::CommandResult { cmd_id, exit, stdout, stderr, done } => {
                CommandResult { cmd_id, exit, stdout, stderr, done }
            }
            _ => panic!("expected CommandResult variant"),
        }
    }
}

#[derive(Clone, Default)]
pub struct Conns {
    senders: Arc<RwLock<HashMap<String, mpsc::Sender<ServerToAgent>>>>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<CommandResult>>>>,
}

impl Conns {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a connection for `id`, returning the receiver end.
    /// Capacity 32.
    pub async fn register(&self, id: &str) -> mpsc::Receiver<ServerToAgent> {
        let (tx, rx) = mpsc::channel(32);
        self.senders.write().await.insert(id.to_string(), tx);
        rx
    }

    /// Send a message to the connected agent. Returns Err if no connection exists.
    pub async fn send(&self, id: &str, msg: ServerToAgent) -> anyhow::Result<()> {
        let senders = self.senders.read().await;
        let tx = senders.get(id)
            .ok_or_else(|| anyhow::anyhow!("no connection for {id}"))?;
        tx.send(msg).await.map_err(|_| anyhow::anyhow!("send failed: receiver dropped for {id}"))?;
        Ok(())
    }

    /// Remove the connection entry for `id`.
    pub async fn unregister(&self, id: &str) {
        self.senders.write().await.remove(id);
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
        let mut rx = conns.register("m-1").await;
        conns.send("m-1", ServerToAgent::Ping).await.unwrap();
        assert!(rx.recv().await.is_some());
        conns.unregister("m-1").await;
        assert!(conns.send("m-1", ServerToAgent::Ping).await.is_err());
    }

    #[tokio::test]
    async fn pending_result_roundtrip() {
        let conns = Conns::new();
        let waiter = conns.new_pending("c-1").await;
        conns.resolve("c-1", sample_result()).await;
        assert_eq!(waiter.await.unwrap().cmd_id, "c-1");
    }
}
