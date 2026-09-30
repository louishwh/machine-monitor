use crate::conn::{CommandResult, Conns};
use fw_proto::messages::ServerToAgent;
use std::time::Duration;

/// Extract the `cmd_id` embedded in a `ServerToAgent` message.
/// Returns `None` for messages that carry no cmd_id (Ping, HelloAck, Reject).
fn extract_cmd_id(msg: &ServerToAgent) -> Option<&str> {
    match msg {
        ServerToAgent::RunStatus { cmd_id, .. } => Some(cmd_id.as_str()),
        ServerToAgent::RunShell { cmd_id, .. } => Some(cmd_id.as_str()),
        _ => None,
    }
}

/// Send `msg` to the agent identified by `machine_id` and wait up to `timeout`
/// for the agent to reply with a matching `CommandResult`.
///
/// Errors if:
/// - the message carries no cmd_id
/// - the machine has no live connection
/// - the result does not arrive within `timeout`
/// - the oneshot receiver is dropped before resolution
pub async fn dispatch(
    conns: &Conns,
    machine_id: &str,
    msg: ServerToAgent,
    timeout: Duration,
) -> anyhow::Result<CommandResult> {
    let cmd_id = extract_cmd_id(&msg)
        .ok_or_else(|| anyhow::anyhow!("message carries no cmd_id"))?
        .to_string();

    // Register the pending waiter BEFORE sending so we cannot miss the reply.
    let mut waiter = conns.new_pending(&cmd_id).await;

    // Send; if the machine is offline this returns Err immediately.
    conns.send(machine_id, msg).await?;

    // Wait with timeout.
    match tokio::time::timeout(timeout, waiter.recv()).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(_)) => Err(anyhow::anyhow!("pending oneshot dropped before resolution")),
        Err(_elapsed) => Err(anyhow::anyhow!("dispatch timed out after {timeout:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dispatch_times_out_when_offline() {
        let conns = Conns::new();
        // "ghost" has no registered connection — send will fail immediately,
        // which should also surface as an error (not a hang).
        let err = dispatch(
            &conns,
            "ghost",
            ServerToAgent::RunStatus {
                cmd_id: "c-test".into(),
                kind: "host".into(),
                arg: None,
            },
            Duration::from_millis(50),
        )
        .await;
        assert!(err.is_err(), "expected Err for offline machine, got Ok");
        assert_eq!(conns.pending_len(), 0);
    }

    #[tokio::test]
    async fn timeout_and_cancellation_remove_pending_waiters() {
        let conns = Conns::new();
        let (mut rx, _kill, _gen) = conns.register("m-1").await;
        let msg = || ServerToAgent::RunStatus {
            cmd_id: "c-timeout".into(),
            kind: "host".into(),
            arg: None,
        };
        let result = dispatch(&conns, "m-1", msg(), Duration::from_millis(20)).await;
        assert!(result.is_err());
        assert_eq!(conns.pending_len(), 0);
        assert!(rx.recv().await.is_some());

        let cloned = conns.clone();
        let task = tokio::spawn(async move {
            dispatch(
                &cloned,
                "m-1",
                ServerToAgent::RunStatus {
                    cmd_id: "c-cancel".into(),
                    kind: "host".into(),
                    arg: None,
                },
                Duration::from_secs(30),
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(conns.pending_len(), 1);
        task.abort();
        let _ = task.await;
        assert_eq!(conns.pending_len(), 0);
    }
}
