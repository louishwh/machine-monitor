use crate::{dispatch, store, AppState};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use fw_proto::messages::{is_known_status_kind, ServerToAgent};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

pub async fn list_machines(
    State(st): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let machines = store::list_machines(&st.pool)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
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
    Ok(Json(Value::Array(out)))
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
            format!(
                "unknown kind {:?}; valid: host cpu mem disk net proc service",
                q.kind
            ),
        ));
    }
    if q.kind == "service" && q.arg.as_deref().is_none_or(|arg| arg.trim().is_empty()) {
        return Err((StatusCode::BAD_REQUEST, "service kind requires arg".into()));
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

    if result.exit != 0 {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("agent status collector failed: {}", result.stderr),
        ));
    }
    let detail: Value = serde_json::from_str(&result.stdout).map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("agent returned invalid status JSON: {e}"),
        )
    })?;
    if let Some(error) = detail.get("error") {
        return Err((StatusCode::BAD_GATEWAY, error.to_string()));
    }

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

/// DELETE /api/machines/:id — remove visible records and retain a revocation
/// tombstone so the old identity cannot recreate the machine.
/// Also kicks any live WebSocket connection and removes it from the registry.
pub async fn delete_machine(
    State(st): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match store::delete_machine(&st.pool, &id).await {
        Ok(_) => {
            st.conns.kick(&id).await;
            st.registry.mark_offline(&id).await;
            (StatusCode::OK, Json(json!({"ok": true})))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        ),
    }
}

/// GET /api/machines/:id/snapshots?limit=50
pub async fn list_machine_snapshots(
    Path(id): Path<String>,
    Query(q): Query<SnapshotsQuery>,
    State(st): State<AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if !(1..=1000).contains(&q.limit) {
        return Err((StatusCode::BAD_REQUEST, "limit must be 1..=1000".into()));
    }
    let snaps = store::list_snapshots(&st.pool, &id, q.limit)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(
        serde_json::to_value(snaps).unwrap_or(Value::Array(vec![])),
    ))
}
