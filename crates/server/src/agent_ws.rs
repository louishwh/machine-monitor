use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use fw_proto::messages::{AgentToServer, ServerToAgent};
use fw_proto::token::verify_identity;
use futures_util::{SinkExt, StreamExt};
use crate::{conn::CommandResult, store, AppState};

pub async fn handler(ws: WebSocketUpgrade, State(st): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |sock| run(sock, st))
}

async fn run(sock: WebSocket, st: AppState) {
    // Split into independent read (stream) and write (sink) halves so they can be
    // used in separate select! arms without borrow conflicts.
    let (mut sink, mut stream) = sock.split();

    // --- Hello handshake: loop until we get a valid Hello or socket closes ---
    let id: String = loop {
        let msg = match stream.next().await {
            Some(Ok(m)) => m,
            _ => return, // socket closed or error before Hello
        };
        let txt = match msg {
            Message::Text(t) => t.as_str().to_string(),
            Message::Close(_) => return,
            _ => continue,
        };
        let Ok(parsed) = serde_json::from_str::<AgentToServer>(&txt) else { continue };
        match parsed {
            AgentToServer::Hello { identity_token, hostname, os, agent_version } => {
                // Read the shared console public key — reject immediately if unpaired.
                let pk_guard = st.console_pubkey.read().await;
                let Some(pk) = pk_guard.as_ref() else {
                    let _ = send_sink(&mut sink, &ServerToAgent::Reject { reason: "未配对".into() }).await;
                    return;
                };
                let payload = match verify_identity(pk, &identity_token) {
                    Ok(p) => {
                        drop(pk_guard); // release read lock before async work
                        p
                    }
                    Err(_) => {
                        drop(pk_guard);
                        let _ = send_sink(&mut sink, &ServerToAgent::Reject { reason: "身份无效".into() }).await;
                        return;
                    }
                };
                if store::is_revoked(&st.pool, &payload.machine_id).await.unwrap_or(false) {
                    let _ = send_sink(&mut sink, &ServerToAgent::Reject { reason: "已吊销".into() }).await;
                    return;
                }
                let _ = store::upsert_machine(
                    &st.pool, &payload.machine_id, &payload.name, &hostname, &os, &agent_version,
                ).await;
                st.registry.mark_online(&payload.machine_id).await;
                let _ = send_sink(&mut sink, &ServerToAgent::HelloAck { ok: true }).await;
                break payload.machine_id;
            }
            // If we see non-Hello before Hello, ignore and keep waiting
            _ => continue,
        }
    };

    // Register this connection in the Conns table.
    let (mut rx, gen) = st.conns.register(&id).await;

    // --- Main loop: select over (socket read) and (outbound channel recv) ---
    loop {
        tokio::select! {
            // Inbound: messages from agent
            maybe_msg = stream.next() => {
                match maybe_msg {
                    Some(Ok(Message::Text(t))) => {
                        let txt = t.as_str().to_string();
                        let Ok(parsed) = serde_json::from_str::<AgentToServer>(&txt) else { continue };
                        match parsed {
                            AgentToServer::Heartbeat { summary } => {
                                st.registry.mark_online(&id).await;
                                let _ = store::touch_last_seen(&st.pool, &id).await;
                                if let Some(s) = summary {
                                    if let Ok(json) = serde_json::to_string(&s) {
                                        let _ = store::save_snapshot(&st.pool, &id, "summary", &json).await;
                                    }
                                }
                            }
                            AgentToServer::CommandResult { cmd_id, exit, stdout, stderr, done } => {
                                let result = CommandResult { cmd_id: cmd_id.clone(), exit, stdout, stderr, done };
                                st.conns.resolve(&cmd_id, result).await;
                            }
                            AgentToServer::Hello { .. } => {
                                // unexpected duplicate Hello — ignore
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {} // ping/pong/binary — ignore
                    Some(Err(_)) => break,
                }
            }

            // Outbound: messages the server wants to send to this agent
            maybe_outbound = rx.recv() => {
                match maybe_outbound {
                    Some(msg) => {
                        if send_sink(&mut sink, &msg).await.is_err() {
                            break;
                        }
                    }
                    None => break, // channel dropped (server shutting down)
                }
            }
        }
    }

    // Cleanup on disconnect — use generation guard so a stale old-task cleanup
    // cannot evict a newer connection that registered with a different gen.
    st.conns.unregister_gen(&id, gen).await;
    st.registry.mark_offline(&id).await;
}

async fn send_sink(
    sink: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    msg: &ServerToAgent,
) -> anyhow::Result<()> {
    let json = serde_json::to_string(msg)?;
    sink.send(Message::text(json)).await?;
    Ok(())
}
