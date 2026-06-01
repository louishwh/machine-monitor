use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use fw_proto::messages::{AgentToServer, ServerToAgent};
use fw_proto::token::verify_identity;
use crate::{store, AppState};

pub async fn handler(ws: WebSocketUpgrade, State(st): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |sock| run(sock, st))
}

async fn run(mut sock: WebSocket, st: AppState) {
    let mut machine_id: Option<String> = None;

    while let Some(Ok(msg)) = sock.recv().await {
        let txt = match msg {
            Message::Text(t) => t.as_str().to_string(),
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(parsed) = serde_json::from_str::<AgentToServer>(&txt) else { continue };
        match parsed {
            AgentToServer::Hello { identity_token, hostname, os, agent_version } => {
                let payload = match verify_identity(&st.console_pubkey, &identity_token) {
                    Ok(p) => p,
                    Err(_) => {
                        let _ = send(&mut sock, &ServerToAgent::Reject { reason: "身份无效".into() }).await;
                        break;
                    }
                };
                if store::is_revoked(&st.pool, &payload.machine_id).await.unwrap_or(false) {
                    let _ = send(&mut sock, &ServerToAgent::Reject { reason: "已吊销".into() }).await;
                    break;
                }
                let _ = store::upsert_machine(
                    &st.pool, &payload.machine_id, &payload.name, &hostname, &os, &agent_version,
                ).await;
                st.registry.mark_online(&payload.machine_id).await;
                machine_id = Some(payload.machine_id.clone());
                let _ = send(&mut sock, &ServerToAgent::HelloAck { ok: true }).await;
            }
            AgentToServer::Heartbeat { .. } => {
                if let Some(id) = &machine_id {
                    st.registry.mark_online(id).await;
                    let _ = store::touch_last_seen(&st.pool, id).await;
                }
            }
            AgentToServer::CommandResult { .. } => { /* M4 */ }
        }
    }
    if let Some(id) = machine_id {
        st.registry.mark_offline(&id).await;
    }
}

async fn send(sock: &mut WebSocket, msg: &ServerToAgent) -> anyhow::Result<()> {
    let json = serde_json::to_string(msg)?;
    sock.send(Message::text(json)).await?;
    Ok(())
}
