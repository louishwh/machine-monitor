# FleetWatch v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 一个机器监控平台：apt/launchd 安装的 Rust agent 持管理端签发的身份令牌反向长连到中心服务端，操作者用 Tauri PC 管理端（唯一入口）查看状态、下发命令。

**Architecture:** Cargo workspace 单仓。`proto` 共享 WS 协议与身份令牌；`server`（Axum）做 agent WS Hub + 验签 + 控制面 API（单端口 `/agent`、`/api`）；`agent`（单二进制）反连上报；`console`（Tauri）是信任根，签发机器身份并作为唯一控制入口。SQLite 存储，反代终止 TLS。

**Tech Stack:** Rust、Axum、tokio、tokio-tungstenite、sqlx/SQLite、ed25519-dalek、base64、sysinfo、Tauri 2 + React/TS。

---

## 里程碑拆分（每个产出可独立验证的软件）

1. **M1 — agent↔server 上线链路**（本计划详细展开）：proto + server hub + 验签 + agent 反连 + 心跳 + 在线可见。产出：签发测试令牌→agent 上线→`/api/machines` 能看到在线。
2. **M2 — 状态采集**：agent 内置 sysinfo 采集器；心跳带轻量摘要；详情按需拉取（server 下发 `RunStatus`，agent 回 `CommandResult`）。
3. **M3 — Tauri 管理端 + 身份签发 + 配对**：管理端主密钥生成、与 server 配对（pairing_token）、签发机器身份令牌、机器列表/在线/详情 UI。
4. **M4 — 命令通道与审计**：状态快捷查询 UI；受控 shell（逐机开启、30s 超时、全量审计）；吊销。
5. **M5 — 打包与运维**：Ubuntu `.deb`(cargo-deb+systemd)、macOS launchd、console bundler、状态快照 1 个月清理任务。

> M2–M5 在本文件末尾给出结构化分解（文件 + 职责 + 测试意图 + 关键签名），执行到该里程碑时展开为逐步 TDD 任务。

---

## 文件结构（v1 全貌）

```
monitor/
  Cargo.toml                      # workspace
  crates/
    proto/
      Cargo.toml
      src/lib.rs                  # 导出 token + messages
      src/token.rs                # IdentityToken 编解码 + sign/verify (ed25519)
      src/messages.rs             # AgentToServer / ServerToAgent 枚举 + 状态类型
    server/
      Cargo.toml
      src/main.rs                 # 启动：加载 config、init db、build router、serve
      src/config.rs               # ServerConfig (bind, console_public_key, db_path)
      src/db.rs                    # SQLite pool + migrations
      src/store.rs                # machines 表 CRUD
      src/registry.rs             # 在线连接池（内存 map）
      src/agent_ws.rs             # /agent WS 处理：验签→注册→心跳
      src/api.rs                  # /api 控制面（M1: GET /api/machines）
      src/sweeper.rs              # 离线判定后台任务
    agent/
      Cargo.toml
      src/main.rs                 # CLI: run / enroll；加载 config 后进 run_loop
      src/config.rs               # AgentConfig (server_url, identity_token)
      src/client.rs               # ws 反连 + Hello + 心跳 + 重连退避
  console/                        # M3 起（Tauri）
  packaging/                      # M5
  docs/
```

---

## M1 — agent↔server 上线链路

### Task 1: Cargo workspace 骨架

**Files:**
- Create: `Cargo.toml`
- Create: `crates/proto/Cargo.toml`, `crates/proto/src/lib.rs`
- Create: `crates/server/Cargo.toml`, `crates/server/src/main.rs`
- Create: `crates/agent/Cargo.toml`, `crates/agent/src/main.rs`

- [ ] **Step 1: workspace 清单**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/proto", "crates/server", "crates/agent"]

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "signal"] }
anyhow = "1"
thiserror = "1"
ed25519-dalek = "2"
base64 = "0.22"
chrono = { version = "0.4", features = ["serde"] }
```

- [ ] **Step 2: proto 清单与空 lib**

`crates/proto/Cargo.toml`:
```toml
[package]
name = "fw-proto"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
ed25519-dalek = { workspace = true }
base64 = { workspace = true }
thiserror = { workspace = true }
chrono = { workspace = true }
```
`crates/proto/src/lib.rs`:
```rust
pub mod token;
pub mod messages;
```

- [ ] **Step 3: server / agent 清单 + 占位 main**

`crates/server/Cargo.toml`:
```toml
[package]
name = "fleetwatch-server"
version = "0.1.0"
edition = "2021"

[dependencies]
fw-proto = { path = "../proto" }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
chrono = { workspace = true }
ed25519-dalek = { workspace = true }
base64 = { workspace = true }
axum = { version = "0.8", features = ["ws"] }
tower-http = { version = "0.6", features = ["trace"] }
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite"] }
tracing = "0.1"
tracing-subscriber = "0.3"

[dev-dependencies]
tokio-tungstenite = "0.24"
reqwest = { version = "0.12", default-features = false, features = ["json"] }
```
`crates/server/src/main.rs`:
```rust
fn main() {
    println!("fleetwatch-server placeholder");
}
```
`crates/agent/Cargo.toml`:
```toml
[package]
name = "fleetwatch-agent"
version = "0.1.0"
edition = "2021"

[dependencies]
fw-proto = { path = "../proto" }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
anyhow = { workspace = true }
chrono = { workspace = true }
tokio-tungstenite = "0.24"
futures-util = "0.3"
toml = "0.8"
clap = { version = "4", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = "0.3"
```
`crates/agent/src/main.rs`:
```rust
fn main() {
    println!("fleetwatch-agent placeholder");
}
```

- [ ] **Step 4: 验证构建**

Run: `cargo build --workspace`
Expected: 三个 crate 全部编译通过。

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/
git commit -m "chore: cargo workspace skeleton (proto/server/agent)"
```

---

### Task 2: proto — 身份令牌 sign/verify

**Files:**
- Create: `crates/proto/src/token.rs`

- [ ] **Step 1: 写失败测试**

`crates/proto/src/token.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn sign_then_verify_roundtrip() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let vk = sk.verifying_key();
        let payload = IdentityPayload {
            machine_id: "m-1".into(),
            name: "web-01".into(),
            issued_at: "2026-05-30T00:00:00Z".into(),
        };
        let token = sign_identity(&sk, &payload);
        let got = verify_identity(vk.as_bytes(), &token).expect("verify ok");
        assert_eq!(got.machine_id, "m-1");
    }

    #[test]
    fn tampered_token_is_rejected() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let vk = sk.verifying_key();
        let payload = IdentityPayload { machine_id: "m-1".into(), name: "x".into(), issued_at: "t".into() };
        let mut token = sign_identity(&sk, &payload);
        token.push('A'); // corrupt signature
        assert!(verify_identity(vk.as_bytes(), &token).is_err());
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p fw-proto token::`
Expected: FAIL — `IdentityPayload` / `sign_identity` / `verify_identity` 未定义。

- [ ] **Step 3: 实现**

在 `crates/proto/src/token.rs` 顶部：
```rust
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, SigningKey, Signer, VerifyingKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityPayload {
    pub machine_id: String,
    pub name: String,
    pub issued_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("格式错误")] Format,
    #[error("base64 解码失败")] B64,
    #[error("签名验证失败")] Sig,
    #[error("payload 解析失败")] Payload,
    #[error("公钥无效")] Key,
}

/// token = base64(payload_json).base64(sig)
pub fn sign_identity(sk: &SigningKey, payload: &IdentityPayload) -> String {
    let bytes = serde_json::to_vec(payload).expect("serialize payload");
    let sig = sk.sign(&bytes);
    format!("{}.{}", B64.encode(&bytes), B64.encode(sig.to_bytes()))
}

pub fn verify_identity(pubkey: &[u8], token: &str) -> Result<IdentityPayload, TokenError> {
    let (p_b64, s_b64) = token.split_once('.').ok_or(TokenError::Format)?;
    let p = B64.decode(p_b64.trim()).map_err(|_| TokenError::B64)?;
    let s = B64.decode(s_b64.trim()).map_err(|_| TokenError::B64)?;
    let key_arr: [u8; 32] = pubkey.try_into().map_err(|_| TokenError::Key)?;
    let vk = VerifyingKey::from_bytes(&key_arr).map_err(|_| TokenError::Key)?;
    let sig = Signature::from_slice(&s).map_err(|_| TokenError::Sig)?;
    vk.verify_strict(&p, &sig).map_err(|_| TokenError::Sig)?;
    serde_json::from_slice(&p).map_err(|_| TokenError::Payload)
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p fw-proto token::`
Expected: PASS（2 个测试）。

- [ ] **Step 5: Commit**

```bash
git add crates/proto/src/token.rs
git commit -m "feat(proto): ed25519 identity token sign/verify"
```

---

### Task 3: proto — WS 消息与状态类型

**Files:**
- Create: `crates/proto/src/messages.rs`

- [ ] **Step 1: 写失败测试**

`crates/proto/src/messages.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hello_roundtrips_through_json() {
        let msg = AgentToServer::Hello {
            identity_token: "tok".into(),
            hostname: "web-01".into(),
            os: "ubuntu".into(),
            agent_version: "0.1.0".into(),
        };
        let s = serde_json::to_string(&msg).unwrap();
        let back: AgentToServer = serde_json::from_str(&s).unwrap();
        matches!(back, AgentToServer::Hello { .. });
    }

    #[test]
    fn server_runstatus_tag_is_stable() {
        let s = serde_json::to_string(&ServerToAgent::Ping).unwrap();
        assert!(s.contains("ping"));
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p fw-proto messages::`
Expected: FAIL — 类型未定义。

- [ ] **Step 3: 实现**

`crates/proto/src/messages.rs` 顶部：
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusSummary {
    pub cpu_pct: f32,
    pub mem_pct: f32,
    pub disk_pct: f32,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentToServer {
    Hello { identity_token: String, hostname: String, os: String, agent_version: String },
    Heartbeat { summary: Option<StatusSummary> },
    CommandResult { cmd_id: String, exit: i32, stdout: String, stderr: String, done: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerToAgent {
    HelloAck { ok: bool },
    Reject { reason: String },
    RunStatus { cmd_id: String, kind: String, arg: Option<String> },
    RunShell { cmd_id: String, command: String },
    Ping,
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p fw-proto`
Expected: PASS（token + messages 全绿）。

- [ ] **Step 5: Commit**

```bash
git add crates/proto/src/messages.rs
git commit -m "feat(proto): ws message + status summary types"
```

---

### Task 4: server — 配置加载

**Files:**
- Create: `crates/server/src/config.rs`
- Modify: `crates/server/src/main.rs`

- [ ] **Step 1: 写失败测试**

`crates/server/src/config.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_pubkey_from_base64() {
        let cfg = ServerConfig {
            bind: "127.0.0.1:8080".into(),
            console_public_key_b64: base64::engine::general_purpose::STANDARD
                .encode([9u8; 32]),
            db_path: "/tmp/fw.db".into(),
        };
        let key = cfg.console_public_key().unwrap();
        assert_eq!(key.len(), 32);
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p fleetwatch-server config::`
Expected: FAIL — `ServerConfig` 未定义。

- [ ] **Step 3: 实现**

`crates/server/src/config.rs`:
```rust
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    /// 管理端公钥（信任锚），首次配对后由 console 注册；M1 先从配置读
    pub console_public_key_b64: String,
    pub db_path: String,
}

impl ServerConfig {
    pub fn console_public_key(&self) -> anyhow::Result<Vec<u8>> {
        let v = B64.decode(self.console_public_key_b64.trim())?;
        anyhow::ensure!(v.len() == 32, "公钥必须为 32 字节");
        Ok(v)
    }
    pub fn from_env_or_file() -> anyhow::Result<Self> {
        let path = std::env::var("FW_SERVER_CONFIG")
            .unwrap_or_else(|_| "server.toml".into());
        let text = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&text)?)
    }
}
```
在 `crates/server/Cargo.toml` 的 `[dependencies]` 增加 `toml = "0.8"`。`main.rs` 暂保留占位（Task 8 重写）。

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p fleetwatch-server config::`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/config.rs crates/server/Cargo.toml
git commit -m "feat(server): config loading with console public key"
```

---

### Task 5: server — SQLite + machines 存储

**Files:**
- Create: `crates/server/src/db.rs`
- Create: `crates/server/src/store.rs`

- [ ] **Step 1: 写失败测试**

`crates/server/src/store.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[tokio::test]
    async fn upsert_and_list_machine() {
        let pool = db::init_pool_in_memory().await.unwrap();
        upsert_machine(&pool, "m-1", "web-01", "web-01.local", "ubuntu", "0.1.0")
            .await.unwrap();
        let list = list_machines(&pool).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "web-01");
        // re-upsert updates, not duplicates
        upsert_machine(&pool, "m-1", "web-01b", "h", "ubuntu", "0.1.0").await.unwrap();
        assert_eq!(list_machines(&pool).await.unwrap().len(), 1);
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p fleetwatch-server store::`
Expected: FAIL — `db`/`store` 未定义。

- [ ] **Step 3: 实现**

`crates/server/src/db.rs`:
```rust
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS machines (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  hostname TEXT NOT NULL DEFAULT '',
  os TEXT NOT NULL DEFAULT '',
  agent_version TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active',
  shell_enabled INTEGER NOT NULL DEFAULT 0,
  last_seen TEXT,
  enrolled_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS revocations (
  machine_id TEXT PRIMARY KEY, revoked_at TEXT NOT NULL
);
"#;

pub async fn init_pool(db_path: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{db_path}"))?
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new().max_connections(4).connect_with(opts).await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

pub async fn init_pool_in_memory() -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")?;
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    for stmt in SCHEMA.split(';') {
        let s = stmt.trim();
        if !s.is_empty() { sqlx::query(s).execute(pool).await?; }
    }
    Ok(())
}
```
`crates/server/src/store.rs`（测试模块上方）：
```rust
use sqlx::{Row, SqlitePool};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Machine {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub os: String,
    pub agent_version: String,
    pub status: String,
    pub last_seen: Option<String>,
}

pub async fn upsert_machine(
    pool: &SqlitePool, id: &str, name: &str, hostname: &str, os: &str, ver: &str,
) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO machines (id,name,hostname,os,agent_version,last_seen,enrolled_at)
         VALUES (?,?,?,?,?,?,?)
         ON CONFLICT(id) DO UPDATE SET
           name=excluded.name, hostname=excluded.hostname, os=excluded.os,
           agent_version=excluded.agent_version, last_seen=excluded.last_seen",
    )
    .bind(id).bind(name).bind(hostname).bind(os).bind(ver).bind(&now).bind(&now)
    .execute(pool).await?;
    Ok(())
}

pub async fn touch_last_seen(pool: &SqlitePool, id: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE machines SET last_seen=? WHERE id=?")
        .bind(&now).bind(id).execute(pool).await?;
    Ok(())
}

pub async fn is_revoked(pool: &SqlitePool, id: &str) -> anyhow::Result<bool> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM revocations WHERE machine_id=?")
        .bind(id).fetch_one(pool).await?;
    Ok(n > 0)
}

pub async fn list_machines(pool: &SqlitePool) -> anyhow::Result<Vec<Machine>> {
    let rows = sqlx::query(
        "SELECT id,name,hostname,os,agent_version,status,last_seen FROM machines ORDER BY name",
    ).fetch_all(pool).await?;
    Ok(rows.into_iter().map(|r| Machine {
        id: r.get("id"), name: r.get("name"), hostname: r.get("hostname"),
        os: r.get("os"), agent_version: r.get("agent_version"),
        status: r.get("status"), last_seen: r.get("last_seen"),
    }).collect())
}
```
在 `main.rs` 顶部声明模块：`mod config; mod db; mod store;`（占位 main 保留）。

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p fleetwatch-server store::`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/db.rs crates/server/src/store.rs crates/server/src/main.rs
git commit -m "feat(server): sqlite pool + machines store"
```

---

### Task 6: server — 在线连接池 registry

**Files:**
- Create: `crates/server/src/registry.rs`

- [ ] **Step 1: 写失败测试**

`crates/server/src/registry.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn mark_online_offline() {
        let reg = Registry::new();
        reg.mark_online("m-1").await;
        assert!(reg.is_online("m-1").await);
        reg.mark_offline("m-1").await;
        assert!(!reg.is_online("m-1").await);
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p fleetwatch-server registry::`
Expected: FAIL — `Registry` 未定义。

- [ ] **Step 3: 实现**

`crates/server/src/registry.rs`:
```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub struct Registry {
    online: Arc<RwLock<HashMap<String, chrono::DateTime<chrono::Utc>>>>,
}

impl Registry {
    pub fn new() -> Self { Self::default() }
    pub async fn mark_online(&self, id: &str) {
        self.online.write().await.insert(id.to_string(), chrono::Utc::now());
    }
    pub async fn mark_offline(&self, id: &str) {
        self.online.write().await.remove(id);
    }
    pub async fn is_online(&self, id: &str) -> bool {
        self.online.read().await.contains_key(id)
    }
    pub async fn online_ids(&self) -> Vec<String> {
        self.online.read().await.keys().cloned().collect()
    }
}
```
在 `main.rs` 增加 `mod registry;`。

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p fleetwatch-server registry::`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/registry.rs crates/server/src/main.rs
git commit -m "feat(server): in-memory online registry"
```

---

### Task 7: server — /agent WS 处理 + /api/machines + 应用装配

**Files:**
- Create: `crates/server/src/agent_ws.rs`
- Create: `crates/server/src/api.rs`
- Modify: `crates/server/src/main.rs`

- [ ] **Step 1: 写失败的集成测试**

`crates/server/tests/online_flow.rs`:
```rust
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::SigningKey;
use fw_proto::token::{sign_identity, IdentityPayload};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn agent_hello_marks_machine_online() {
    let sk = SigningKey::from_bytes(&[3u8; 32]);
    let pubkey_b64 = B64.encode(sk.verifying_key().as_bytes());

    // 启动 server（in-memory db），监听 127.0.0.1:0
    let addr = fleetwatch_server::test_support::spawn_test_server(pubkey_b64).await;

    // agent 端：签发身份令牌并连 ws
    let token = sign_identity(&sk, &IdentityPayload {
        machine_id: "m-1".into(), name: "web-01".into(),
        issued_at: chrono::Utc::now().to_rfc3339(),
    });
    let url = format!("ws://{addr}/agent");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let hello = serde_json::json!({
        "type":"hello","identity_token":token,"hostname":"web-01",
        "os":"ubuntu","agent_version":"0.1.0"
    });
    ws.send(Message::Text(hello.to_string())).await.unwrap();
    // 读 HelloAck
    let reply = ws.next().await.unwrap().unwrap();
    assert!(reply.into_text().unwrap().contains("hello_ack"));

    // 查 /api/machines 应见 online
    let body: serde_json::Value = reqwest::get(format!("http://{addr}/api/machines"))
        .await.unwrap().json().await.unwrap();
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["online"], true);
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p fleetwatch-server --test online_flow`
Expected: FAIL — `test_support::spawn_test_server` / 路由未定义。

- [ ] **Step 3: 实现 AppState + WS + API + test_support**

`crates/server/src/main.rs`（重写为 lib+bin 结构，导出 `test_support`）：
```rust
mod config; mod db; mod store; mod registry; mod agent_ws; mod api; mod sweeper;

use axum::{routing::get, Router};
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub registry: registry::Registry,
    pub console_pubkey: Arc<Vec<u8>>,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/agent", get(agent_ws::handler))
        .route("/api/machines", get(api::list_machines))
        .with_state(state)
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::*;
    pub async fn spawn_test_server(pubkey_b64: String) -> String {
        let pool = db::init_pool_in_memory().await.unwrap();
        let console_pubkey = Arc::new(
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, pubkey_b64).unwrap()
        );
        let state = AppState { pool, registry: registry::Registry::new(), console_pubkey };
        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        addr
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cfg = config::ServerConfig::from_env_or_file()?;
    let pool = db::init_pool(&cfg.db_path).await?;
    let console_pubkey = Arc::new(cfg.console_public_key()?);
    let state = AppState { pool, registry: registry::Registry::new(), console_pubkey };
    sweeper::spawn(state.clone());
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    tracing::info!(bind = %cfg.bind, "fleetwatch-server listening");
    axum::serve(listener, app).await?;
    Ok(())
}
```
为让集成测试访问 `fleetwatch_server::...`，在 `crates/server/Cargo.toml` 增加 lib 目标：
```toml
[lib]
name = "fleetwatch_server"
path = "src/lib.rs"
```
并把上面的模块声明/AppState/build_router/test_support 移到 `src/lib.rs`，`src/main.rs` 改为：
```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> { fleetwatch_server::run().await }
```
（将 `main` 体改写为 `pub async fn run()` 放进 `lib.rs`。）

`crates/server/src/agent_ws.rs`:
```rust
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
        let Message::Text(txt) = msg else { continue };
        let Ok(parsed) = serde_json::from_str::<AgentToServer>(&txt) else { continue };
        match parsed {
            AgentToServer::Hello { identity_token, hostname, os, agent_version } => {
                let payload = match verify_identity(&st.console_pubkey, &identity_token) {
                    Ok(p) => p,
                    Err(_) => { let _ = send(&mut sock, &ServerToAgent::Reject{reason:"身份无效".into()}).await; break; }
                };
                if store::is_revoked(&st.pool, &payload.machine_id).await.unwrap_or(false) {
                    let _ = send(&mut sock, &ServerToAgent::Reject{reason:"已吊销".into()}).await; break;
                }
                let _ = store::upsert_machine(&st.pool, &payload.machine_id, &payload.name, &hostname, &os, &agent_version).await;
                st.registry.mark_online(&payload.machine_id).await;
                machine_id = Some(payload.machine_id.clone());
                let _ = send(&mut sock, &ServerToAgent::HelloAck{ok:true}).await;
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
    if let Some(id) = machine_id { st.registry.mark_offline(&id).await; }
}

async fn send(sock: &mut WebSocket, msg: &ServerToAgent) -> anyhow::Result<()> {
    sock.send(Message::Text(serde_json::to_string(msg)?)).await?;
    Ok(())
}
```
`crates/server/src/api.rs`:
```rust
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
            "id": m.id, "name": m.name, "hostname": m.hostname, "os": m.os,
            "agentVersion": m.agent_version, "status": m.status,
            "lastSeen": m.last_seen, "online": online
        }));
    }
    Json(Value::Array(out))
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p fleetwatch-server --test online_flow`
Expected: PASS — 集成测试看到机器 online。

- [ ] **Step 5: Commit**

```bash
git add crates/server/
git commit -m "feat(server): /agent ws hello+heartbeat, /api/machines, online flow test"
```

---

### Task 8: server — 离线 sweeper

**Files:**
- Create: `crates/server/src/sweeper.rs`

- [ ] **Step 1: 写失败测试**

`crates/server/src/sweeper.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;
    #[tokio::test]
    async fn stale_entries_go_offline() {
        let reg = Registry::new();
        reg.mark_online_at("m-1", chrono::Utc::now() - chrono::Duration::seconds(90)).await;
        reg.mark_online("m-2").await;
        sweep_once(&reg, 45).await;
        assert!(!reg.is_online("m-1").await);
        assert!(reg.is_online("m-2").await);
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p fleetwatch-server sweeper::`
Expected: FAIL — `sweep_once` / `mark_online_at` 未定义。

- [ ] **Step 3: 实现**

在 `registry.rs` 增加：
```rust
impl Registry {
    pub async fn mark_online_at(&self, id: &str, at: chrono::DateTime<chrono::Utc>) {
        self.online.write().await.insert(id.to_string(), at);
    }
    pub async fn last_seen(&self, id: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        self.online.read().await.get(id).copied()
    }
    pub async fn snapshot(&self) -> Vec<(String, chrono::DateTime<chrono::Utc>)> {
        self.online.read().await.iter().map(|(k,v)|(k.clone(),*v)).collect()
    }
}
```
`crates/server/src/sweeper.rs`:
```rust
use crate::{registry::Registry, AppState};

pub async fn sweep_once(reg: &Registry, timeout_secs: i64) {
    let now = chrono::Utc::now();
    for (id, seen) in reg.snapshot().await {
        if (now - seen).num_seconds() > timeout_secs {
            reg.mark_offline(&id).await;
        }
    }
}

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));
        loop { tick.tick().await; sweep_once(&state.registry, 45).await; }
    });
}
```
在 `lib.rs` 增加 `mod sweeper;`。

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p fleetwatch-server sweeper::`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/server/src/sweeper.rs crates/server/src/registry.rs
git commit -m "feat(server): offline sweeper (45s heartbeat timeout)"
```

---

### Task 9: agent — 配置加载

**Files:**
- Create: `crates/agent/src/config.rs`

- [ ] **Step 1: 写失败测试**

`crates/agent/src/config.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_toml() {
        let toml = r#"
            server_url = "wss://mon.example.com/agent"
            identity_token = "abc.def"
        "#;
        let c: AgentConfig = toml::from_str(toml).unwrap();
        assert_eq!(c.server_url, "wss://mon.example.com/agent");
        assert_eq!(c.identity_token, "abc.def");
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p fleetwatch-agent config::`
Expected: FAIL — `AgentConfig` 未定义。

- [ ] **Step 3: 实现**

`crates/agent/src/config.rs`:
```rust
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    pub server_url: String,
    pub identity_token: String,
}

impl AgentConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
    }
}
```
`main.rs` 增加 `mod config;`（占位 main 保留）。

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p fleetwatch-agent config::`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/agent/src/config.rs crates/agent/src/main.rs
git commit -m "feat(agent): toml config loading"
```

---

### Task 10: agent — 重连退避 + ws 客户端 + CLI

**Files:**
- Create: `crates/agent/src/client.rs`
- Modify: `crates/agent/src/main.rs`

- [ ] **Step 1: 写失败测试（退避逻辑，纯函数）**

`crates/agent/src/client.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(next_backoff(1), 2);
        assert_eq!(next_backoff(2), 4);
        assert_eq!(next_backoff(20), 30); // capped at 30
    }
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test -p fleetwatch-agent client::`
Expected: FAIL — `next_backoff` 未定义。

- [ ] **Step 3: 实现 client + CLI**

`crates/agent/src/client.rs`:
```rust
use std::time::Duration;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use fw_proto::messages::{AgentToServer, ServerToAgent};
use crate::config::AgentConfig;

pub fn next_backoff(curr: u64) -> u64 { (curr * 2).min(30) }

fn detect_os() -> String {
    if cfg!(target_os = "macos") { "macos".into() } else { "ubuntu".into() }
}

pub async fn run_loop(cfg: AgentConfig) {
    let mut backoff = 1u64;
    loop {
        match connect_once(&cfg).await {
            Ok(()) => backoff = 1,
            Err(e) => tracing::warn!(error=%e, "agent session ended"),
        }
        tokio::time::sleep(Duration::from_secs(backoff)).await;
        backoff = next_backoff(backoff);
    }
}

async fn connect_once(cfg: &AgentConfig) -> anyhow::Result<()> {
    let (mut ws, _) = tokio_tungstenite::connect_async(&cfg.server_url).await?;
    let hostname = hostname();
    let hello = AgentToServer::Hello {
        identity_token: cfg.identity_token.clone(),
        hostname, os: detect_os(), agent_version: env!("CARGO_PKG_VERSION").into(),
    };
    ws.send(Message::Text(serde_json::to_string(&hello)?)).await?;

    let mut hb = tokio::time::interval(Duration::from_secs(15));
    loop {
        tokio::select! {
            _ = hb.tick() => {
                let m = AgentToServer::Heartbeat { summary: None };
                ws.send(Message::Text(serde_json::to_string(&m)?)).await?;
            }
            msg = ws.next() => {
                let Some(msg) = msg else { break };
                let Message::Text(txt) = msg? else { continue };
                if let Ok(ServerToAgent::Ping) = serde_json::from_str(&txt) { /* keepalive */ }
                // RunStatus / RunShell 处理在 M2/M4
            }
        }
    }
    Ok(())
}

fn hostname() -> String {
    std::process::Command::new("hostname").output().ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}
```
`crates/agent/src/main.rs`:
```rust
mod config; mod client;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name="fleetwatch-agent")]
struct Cli { #[command(subcommand)] cmd: Cmd }

#[derive(Subcommand)]
enum Cmd {
    /// 以配置文件运行（默认 /etc/fleetwatch/agent.toml）
    Run { #[arg(long, default_value="/etc/fleetwatch/agent.toml")] config: String },
    /// 写入身份令牌到配置
    Enroll {
        #[arg(long)] server: String,
        #[arg(long)] identity: String,
        #[arg(long, default_value="/etc/fleetwatch/agent.toml")] config: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    match Cli::parse().cmd {
        Cmd::Run { config } => {
            let cfg = config::AgentConfig::load(&config)?;
            client::run_loop(cfg).await;
        }
        Cmd::Enroll { server, identity, config } => {
            let body = format!("server_url = \"{server}\"\nidentity_token = \"{identity}\"\n");
            if let Some(p) = std::path::Path::new(&config).parent() { std::fs::create_dir_all(p).ok(); }
            std::fs::write(&config, body)?;
            println!("已写入 {config}");
        }
    }
    Ok(())
}
```

- [ ] **Step 4: 运行验证通过**

Run: `cargo test -p fleetwatch-agent client::` 然后 `cargo build --workspace`
Expected: 测试 PASS；workspace 构建通过。

- [ ] **Step 5: Commit**

```bash
git add crates/agent/
git commit -m "feat(agent): ws reverse-connect client + heartbeat + enroll CLI"
```

---

### Task 11: 端到端冒烟（手动验证脚本）

**Files:**
- Create: `scripts/smoke-m1.sh`

- [ ] **Step 1: 写脚本**

`scripts/smoke-m1.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail
# 用 license-style 工具生成测试密钥对与令牌的最小验证：依赖一个一次性 Rust 助手
# 这里直接用集成测试覆盖；本脚本仅启动 server + agent 做人工观察。
echo "请先在 server.toml 填入 console_public_key_b64（与签发令牌的私钥匹配）"
echo "1) cargo run -p fleetwatch-server"
echo "2) fleetwatch-agent enroll --server ws://127.0.0.1:8080/agent --identity <token> --config /tmp/agent.toml"
echo "3) fleetwatch-agent run --config /tmp/agent.toml"
echo "4) curl http://127.0.0.1:8080/api/machines  # 应见 online:true"
```

- [ ] **Step 2: 运行 workspace 全量测试**

Run: `cargo test --workspace`
Expected: 全绿（proto + server 单测/集成 + agent 单测）。

- [ ] **Step 3: Commit**

```bash
git add scripts/smoke-m1.sh
git commit -m "chore: M1 smoke script + workspace tests green"
```

**M1 完成标准：** `cargo test --workspace` 全绿；集成测试证明 agent 持签发令牌反连后 `/api/machines` 显示 `online:true`；伪造/吊销令牌被拒。

---

## M2 — 状态采集（结构化分解）

**Files:** `crates/agent/src/collectors.rs`（sysinfo 采集 host/cpu/mem/disk/net/top进程/服务状态）、修改 `agent/src/client.rs`（Heartbeat 带 `StatusSummary`；处理 `RunStatus` → 采集 → 回 `CommandResult`）、`server/src/agent_ws.rs`（收 `CommandResult` 存 `status_snapshots`）、`server/src/store.rs`（snapshot 增删查 + 1 个月清理）、`server/src/api.rs`（`GET /api/machines/:id/status` 触发按需拉取并返回）。
**测试意图：** 采集器返回合理范围值（mem_pct ∈ [0,100]）；`RunStatus` 往返产出快照；过期快照被清理任务删除。
**关键签名：** `fn collect_summary() -> StatusSummary`、`fn collect_detail(kind: &str, arg: Option<String>) -> serde_json::Value`、`async fn save_snapshot(pool, machine_id, json)`、`async fn purge_old_snapshots(pool, days=30)`。
**依赖：** agent 增加 `sysinfo = "0.32"`。

## M3 — Tauri 管理端 + 身份签发 + 配对（结构化分解）

**Files:** `console/`（Tauri：`src-tauri` Rust 核心 + React UI）。Rust 核心：`master_key.rs`（ed25519 主密钥生成/钥匙串存储，复用 keyring）、`issue.rs`（`sign_identity` 复用 fw-proto 签发机器令牌）、`pairing.rs`（首次用 pairing_token 向 server 注册公钥）、`api_client.rs`（带签名挑战调用 server 控制面）、`commands.rs`（Tauri IPC：generate_master_key/pair_server/issue_machine/list_machines/get_status）。前端：连接设置、机器列表（在线状态）、签发机器（出令牌）、机器详情（状态）。server 侧新增：`POST /api/pair`（一次性 token 注册 console 公钥写入 `server_config`）、控制面鉴权中间件（验证 console 签名挑战）、改 `console_pubkey` 来源为 DB（`server_config`）而非静态配置。
**测试意图：** 配对后 server 存住 console 公钥；签发的令牌能被 server 接受；未配对的控制面请求被拒。
**关键签名：** `fn issue_identity(master: &SigningKey, machine_id, name) -> String`、`async fn pair(server, pairing_token, pubkey)`、`async fn signed_get(server, path, master) -> Value`。

## M4 — 命令通道与审计（结构化分解）

**Files:** `server/src/api.rs`（`POST /api/machines/:id/run-status`、`POST /api/machines/:id/run-shell`、`PATCH /api/machines/:id/shell`、`POST /api/machines/:id/revoke`、`GET /api/audit`）、`server/src/dispatch.rs`（把命令经 registry 的连接句柄下推给在线 agent 并等结果，30s 超时）、`server/src/store.rs`（`command_log` 写入 + 查询；`shell_enabled` 开关；`revocations` 写入）、`agent/src/client.rs`（处理 `RunShell`：仅执行、`sh -c` 带 30s 超时、回传）、console 前端命令控制台 + 审计页。
**测试意图：** shell 关闭时 server 拒发 `RunShell`；开启后命令往返拿到输出且写审计；吊销后该 agent 重连被拒。
**关键签名：** `async fn dispatch(reg, machine_id, ServerToAgent, timeout=30s) -> CommandResult`、`async fn set_shell_enabled(pool,id,bool)`、`async fn revoke(pool,id)`、`async fn log_command(pool, entry)`。
**注意：** registry 需从“仅记 last_seen”升级为“持有每连接的 `mpsc::Sender<ServerToAgent>`”，以便控制面把命令下推到对应 agent 连接；`agent_ws::run` 注册/注销该 sender。

## M5 — 打包与运维（结构化分解）

**Files:** `packaging/deb/`（`cargo-deb` 元数据写进 `agent/Cargo.toml` 的 `[package.metadata.deb]`：systemd unit `fleetwatch-agent.service`、配置模板 `/etc/fleetwatch/agent.toml`、postinst 启用服务）、`packaging/macos/com.fleetwatch.agent.plist`（LaunchDaemon）+ 安装脚本、`console` 的 `tauri build` 出 `.dmg`、`server/src/sweeper.rs` 接入 `purge_old_snapshots` 每日清理。
**测试意图：** `cargo deb -p fleetwatch-agent` 产出 `.deb`；`dpkg -I` 检查 systemd 集成；macOS `launchctl load` 后进程常驻。
**交付脚本：** `scripts/build-deb.sh`、`scripts/install-macos.sh`。

---

## 自检（spec 覆盖）

- 反向长连 / 验签 / 在线可见 → M1 ✅
- 状态采集（内置安全集）+ 按需拉取 + 1 个月保留 → M2 ✅
- 管理端信任根 / 签发身份 / 配对 / 唯一入口 → M3 ✅
- 受控 shell + 审计 + 吊销 + shell 默认关 → M4 ✅
- Ubuntu .deb + macOS launchd + console bundle + 清理任务 → M5 ✅
- 单端口不同路径（/agent、/api）→ M1 路由 ✅
- SQLite / 永久令牌靠吊销 / 30s shell 超时 → 贯穿 ✅
