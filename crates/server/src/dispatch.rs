use std::time::Duration;
use fw_proto::messages::ServerToAgent;
use crate::conn::{CommandResult, Conns};

/// Extract the `cmd_id` embedded in a `ServerToAgent` message.
/// Returns `None` for messages that carry no cmd_id (Ping, HelloAck, Reject).
fn extract_cmd_id(msg: &ServerToAgent) -> Option<&str> {
    match msg {
        ServerToAgent::RunStatus { cmd_id, .. } => Some(cmd_id.as_str()),
        ServerToAgent::RunShell  { cmd_id, .. } => Some(cmd_id.as_str()),
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
    let rx = conns.new_pending(&cmd_id).await;

    // Send; if the machine is offline this returns Err immediately.
    conns.send(machine_id, msg).await?;

    // Wait with timeout.
    match tokio::time::timeout(timeout, rx).await {
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
    }
}
