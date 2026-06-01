use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use crate::{store, AppState};

pub async fn list_machines(State(st): State<AppState>) -> Json<Value> {
    let machines = store::list_machines(&st.pool).await.unwrap_or_default();
    let mut out = Vec::new();
    for m in machines {
        let online = st.registry.is_online(&m.id).await;
        out.push(json!({
            "id": m.id,
            "name": m.name,
            "hostname": m.hostname,
            "os": m.os,
            "agentVersion": m.agent_version,
            "status": m.status,
            "lastSeen": m.last_seen,
            "online": online
        }));
    }
    Json(Value::Array(out))
}
