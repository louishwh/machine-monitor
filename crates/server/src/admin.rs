use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use fw_proto::messages::ServerToAgent;
use crate::{dispatch, store, AppState};

// ── PATCH /api/machines/:id/shell ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ShellToggleBody {
    pub enabled: bool,
}

/// PATCH /api/machines/:id/shell  { "enabled": bool }
/// Enables or disables shell access for the given machine.
pub async fn patch_shell_enabled(
    Path(id): Path<String>,
    State(st): State<AppState>,
    Json(body): Json<ShellToggleBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    store::set_shell_enabled(&st.pool, &id, body.enabled)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({"ok": true})))
}

// ── POST /api/machines/:id/run-shell ─────────────────────────────────────────

#[derive(Deserialize)]
pub struct RunShellBody {
    pub command: String,
}

/// POST /api/machines/:id/run-shell  { "command": "..." }
/// Dispatches a RunShell to the live agent and returns the output.
/// Returns 403 if shell is not enabled, 502 if agent is offline, 504 on timeout.
pub async fn post_run_shell(
    Path(id): Path<String>,
    State(st): State<AppState>,
    Json(body): Json<RunShellBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    // Guard: shell must be explicitly enabled for this machine.
    let enabled = store::get_shell_enabled(&st.pool, &id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !enabled {
        return Err((StatusCode::FORBIDDEN, "shell not enabled for this machine".into()));
    }

    let cmd_id = uuid::Uuid::new_v4().to_string();
    let msg = ServerToAgent::RunShell {
        cmd_id: cmd_id.clone(),
        command: body.command.clone(),
    };

    let result = dispatch::dispatch(&st.conns, &id, msg, Duration::from_secs(30))
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("timed out") {
                (StatusCode::GATEWAY_TIMEOUT, msg)
            } else {
                (StatusCode::BAD_GATEWAY, msg)
            }
        })?;

    // Persist audit log entry.
    let entry = store::CommandLogEntry {
        id: cmd_id,
        machine_id: id.clone(),
        kind: "shell".into(),
        request: body.command.clone(),
        exit: Some(result.exit as i64),
        output: format!("{}{}", result.stdout, result.stderr),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let _ = store::log_command(&st.pool, &entry).await;

    Ok(Json(json!({
        "exit": result.exit,
        "stdout": result.stdout,
        "stderr": result.stderr,
    })))
}

// ── POST /api/machines/:id/revoke ────────────────────────────────────────────

/// POST /api/machines/:id/revoke
/// Adds the machine to the revocations table, kicks any live connection,
/// and marks the machine offline in the in-memory registry.
pub async fn post_revoke(
    Path(id): Path<String>,
    State(st): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    store::add_revocation(&st.pool, &id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    st.conns.kick(&id).await;
    st.registry.mark_offline(&id).await;
    Ok(Json(json!({"ok": true})))
}

// ── GET /api/audit ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AuditQuery {
    pub machine_id: Option<String>,
    pub limit: Option<i64>,
}

/// GET /api/audit?machine_id=&limit=
/// Returns audit log entries (command_log), newest first.
pub async fn get_audit(
    Query(q): Query<AuditQuery>,
    State(st): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let limit = q.limit.unwrap_or(100);
    let rows = store::list_audit(&st.pool, q.machine_id.as_deref(), limit)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::to_value(rows).unwrap_or(Value::Array(vec![]))))
}
