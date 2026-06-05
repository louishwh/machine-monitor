use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use fw_proto::messages::{is_known_status_kind, ServerToAgent};
use crate::{dispatch, store, AppState};

pub async fn list_machines(State(st): State<AppState>) -> Json<Value> {
    let machines = store::list_machines(&st.pool).await.unwrap_or_default();
    let mut out = Vec::new();
    for m in machines {
        let online = st.registry.is_online(&m.id).await;
        // Latest heartbeat summary (cpu/mem/disk %) for at-a-glance list rows.
        let summary = store::latest_snapshot(&st.pool, &m.id, "summary")
            .await
            .ok()
            .flatten()
            .and_then(|(j, _ts)| serde_json::from_str::<Value>(&j).ok());
        out.push(json!({
            "id": m.id,
            "name": m.name,
            "hostname": m.hostname,
            "os": m.os,
            "agentVersion": m.agent_version,
            "status": m.status,
            "lastSeen": m.last_seen,
            "online": online,
            "shellEnabled": m.shell_enabled,
            "summary": summary
        }));
    }
    Json(Value::Array(out))
}

#[derive(Deserialize)]
pub struct StatusQuery {
    pub kind: String,
    pub arg: Option<String>,
}

/// GET /api/machines/:id/status?kind=host[&arg=<svc>]
///
/// Validates `kind`, dispatches a `RunStatus` command to the live agent,
/// persists the returned JSON snapshot, and returns the detail JSON.
pub async fn get_machine_status(
    Path(id): Path<String>,
    Query(q): Query<StatusQuery>,
    State(st): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !is_known_status_kind(&q.kind) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("unknown kind {:?}; valid: host cpu mem disk net proc service", q.kind),
        ));
    }

    let cmd_id = uuid::Uuid::new_v4().to_string();
    let msg = ServerToAgent::RunStatus {
        cmd_id: cmd_id.clone(),
        kind: q.kind.clone(),
        arg: q.arg.clone(),
    };

    let result = dispatch::dispatch(&st.conns, &id, msg, Duration::from_secs(30))
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("dispatch failed: {e}")))?;

    // Parse stdout as JSON; fall back to wrapping it in a string field.
    let detail: Value = serde_json::from_str(&result.stdout)
        .unwrap_or_else(|_| json!({ "raw": result.stdout }));

    // Persist the snapshot asynchronously — ignore errors (best-effort).
    let _ = store::save_snapshot(&st.pool, &id, &q.kind, &result.stdout).await;

    Ok(Json(detail))
}

#[derive(Deserialize)]
pub struct SnapshotsQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_limit() -> i64 {
    50
}

/// DELETE /api/machines/:id — remove machine and all its dependent rows.
/// Also kicks any live WebSocket connection and removes from the in-memory registry.
pub async fn delete_machine(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Best-effort: disconnect any live agent connection and remove from registry.
    st.conns.kick(&id).await;
    st.registry.mark_offline(&id).await;
    match store::delete_machine(&st.pool, &id).await {
        Ok(_) => (StatusCode::OK, Json(json!({"ok": true}))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))),
    }
}

/// GET /api/machines/:id/snapshots?limit=50
pub async fn list_machine_snapshots(
    Path(id): Path<String>,
    Query(q): Query<SnapshotsQuery>,
    State(st): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let snaps = store::list_snapshots(&st.pool, &id, q.limit)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::to_value(snaps).unwrap_or(Value::Array(vec![]))))
}
