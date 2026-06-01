# FleetWatch M2 — 状态采集与按需拉取 实现计划

> **For agentic workers:** 用 superpowers:subagent-driven-development 执行。步骤用 `- [ ]`。

**Goal:** agent 周期心跳带轻量状态摘要并入库；server 能向指定在线 agent 按需下发采集命令并收回详细状态；快照保留 1 个月。

**Architecture:** M1 已有 `/agent` WS、registry、SQLite。M2 把 registry 从“只记 last_seen”升级为“持有每连接的 `mpsc::Sender<ServerToAgent>` + 待回结果的 oneshot 表”，从而 server 可向某 agent 下发 `RunStatus` 并 await `CommandResult`（这是状态与后续 shell 共用的分发底座）。agent 用 `sysinfo` 实现采集器。

**Tech Stack:** Rust、sysinfo、tokio(mpsc/oneshot)、axum、sqlx/SQLite。

---

## 文件结构（M2 改动）

```
crates/agent/src/collectors.rs     (新) sysinfo 采集：summary + detail
crates/agent/src/client.rs         (改) 心跳带 summary；处理 RunStatus→采集→CommandResult
crates/server/src/conn.rs          (新) 连接表：machine_id → Sender<ServerToAgent>；pending oneshot by cmd_id
crates/server/src/dispatch.rs      (新) dispatch(machine_id, msg, timeout)->CommandResult
crates/server/src/agent_ws.rs      (改) 注册/注销 sender + writer 任务；CommandResult 路由到 pending；Heartbeat summary 入库
crates/server/src/store.rs         (改) status_snapshots：save/latest/list/purge
crates/server/src/api.rs           (改) GET /api/machines/:id/status（按需拉详情）；GET /api/machines/:id/snapshots
crates/server/src/db.rs            (改) status_snapshots 表
crates/server/src/sweeper.rs       (改) 每日 purge_old_snapshots(30)
crates/server/src/lib.rs           (改) 新模块声明 + 路由 + AppState 增 conn 表
```

---

### Task 1: proto — 详情请求类型对齐

**Files:** Modify: `crates/proto/src/messages.rs`

M1 的 `ServerToAgent::RunStatus { cmd_id, kind, arg }` 已够用（`kind` 取值：`host|cpu|mem|disk|net|proc|service`，`service` 用 `arg` 传服务名）。本任务仅新增一个 `StatusKind` 常量校验帮助函数与测试，确保 server/agent 对 kind 取值一致。

- [ ] **Step 1: 失败测试**
```rust
#[test]
fn known_status_kinds() {
    assert!(is_known_status_kind("cpu"));
    assert!(is_known_status_kind("service"));
    assert!(!is_known_status_kind("rm-rf"));
}
```
- [ ] **Step 2: 运行失败** `cargo test -p fw-proto known_status_kinds` → FAIL。
- [ ] **Step 3: 实现**（messages.rs 末尾）
```rust
pub const STATUS_KINDS: &[&str] = &["host","cpu","mem","disk","net","proc","service"];
pub fn is_known_status_kind(k: &str) -> bool { STATUS_KINDS.contains(&k) }
```
- [ ] **Step 4: 通过** `cargo test -p fw-proto` → PASS。
- [ ] **Step 5: Commit** `feat(proto): status kind whitelist`

---

### Task 2: agent — 采集器 collectors.rs

**Files:** Create: `crates/agent/src/collectors.rs`; Modify: `crates/agent/Cargo.toml`(+`sysinfo = "0.32"`, `serde_json = { workspace = true }`), `crates/agent/src/main.rs`(`mod collectors;`)

- [ ] **Step 1: 失败测试**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summary_values_in_range() {
        let s = collect_summary();
        assert!(s.cpu_pct >= 0.0 && s.cpu_pct <= 100.0);
        assert!(s.mem_pct >= 0.0 && s.mem_pct <= 100.0);
        assert!(s.disk_pct >= 0.0 && s.disk_pct <= 100.0);
    }
    #[test]
    fn detail_host_has_fields() {
        let v = collect_detail("host", None);
        assert!(v.get("hostname").is_some());
        assert!(v.get("os").is_some());
    }
    #[test]
    fn unknown_kind_returns_error_json() {
        let v = collect_detail("nope", None);
        assert!(v.get("error").is_some());
    }
}
```
- [ ] **Step 2: 运行失败** → FAIL（函数未定义）。
- [ ] **Step 3: 实现** `collect_summary() -> fw_proto::messages::StatusSummary` 与 `collect_detail(kind: &str, arg: Option<String>) -> serde_json::Value`。用 `sysinfo::System`：
  - summary：cpu 全局使用率、内存 used/total、根挂载磁盘用量、uptime。
  - detail：`host`(hostname/os/kernel/uptime)、`cpu`(per-core + load)、`mem`(total/used/swap)、`disk`(每挂载点 mount/total/avail)、`net`(每网卡 rx/tx)、`proc`(top 10 by cpu/mem：pid/name/cpu/mem)、`service`(传 `arg` 服务名：Ubuntu `systemctl is-active <svc>`，macOS `launchctl list | grep`；用 `std::process::Command`，缺失返回 unknown)。
  - 未知 kind：返回 `serde_json::json!({"error": format!("未知采集类型: {kind}")})`。
  实现时注意 sysinfo 0.32 API（`System::new_all()` / `refresh_*` / `cpus()` / `disks()` 经 `sysinfo::Disks::new_with_refreshed_list()` / `networks()` 经 `sysinfo::Networks`）。如某指标在 sysinfo 0.32 取不到，降级为合理占位并注释，不要 panic。
- [ ] **Step 4: 通过** `cargo test -p fleetwatch-agent collectors::` → PASS。
- [ ] **Step 5: Commit** `feat(agent): sysinfo collectors (summary + detail)`

---

### Task 3: agent — 心跳带 summary + 处理 RunStatus

**Files:** Modify: `crates/agent/src/client.rs`

- [ ] **Step 1: 失败测试**（纯函数：把 RunStatus 转为回包）
```rust
#[test]
fn build_status_result_carries_cmd_id() {
    let r = build_status_result("c-1", "host", None);
    match r {
        fw_proto::messages::AgentToServer::CommandResult { cmd_id, done, .. } => {
            assert_eq!(cmd_id, "c-1"); assert!(done);
        }
        _ => panic!("expected CommandResult"),
    }
}
```
- [ ] **Step 2: 运行失败** → FAIL。
- [ ] **Step 3: 实现**
  - 新增 `pub fn build_status_result(cmd_id: &str, kind: &str, arg: Option<String>) -> AgentToServer`：调用 `crate::collectors::collect_detail`，把 JSON 序列化进 `CommandResult{cmd_id, exit:0, stdout:<json string>, stderr:"", done:true}`。
  - `connect_once`：心跳改为 `AgentToServer::Heartbeat { summary: Some(crate::collectors::collect_summary()) }`。
  - 读到 `ServerToAgent::RunStatus{cmd_id,kind,arg}` → `ws.send(build_status_result(...))`。
  - 读到 `RunShell` → M4，暂忽略。
- [ ] **Step 4: 通过** `cargo test -p fleetwatch-agent` + `cargo build -p fleetwatch-agent`。
- [ ] **Step 5: Commit** `feat(agent): heartbeat summary + RunStatus handling`

---

### Task 4: server — 连接表 conn.rs

**Files:** Create: `crates/server/src/conn.rs`; Modify: `lib.rs`(`mod conn;`, `AppState` 增 `conns: Conns`)

- [ ] **Step 1: 失败测试**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use fw_proto::messages::ServerToAgent;
    #[tokio::test]
    async fn register_send_unregister() {
        let conns = Conns::new();
        let mut rx = conns.register("m-1").await;     // 返回接收端
        conns.send("m-1", ServerToAgent::Ping).await.unwrap();
        assert!(rx.recv().await.is_some());
        conns.unregister("m-1").await;
        assert!(conns.send("m-1", ServerToAgent::Ping).await.is_err());
    }

    #[tokio::test]
    async fn pending_result_roundtrip() {
        let conns = Conns::new();
        let waiter = conns.new_pending("c-1").await;   // oneshot Receiver
        conns.resolve("c-1", sample_result()).await;
        assert_eq!(waiter.await.unwrap().cmd_id, "c-1");
    }
}
```
（`sample_result()` 构造一个 `CommandResult{cmd_id:"c-1",..}`。）
- [ ] **Step 2: 运行失败** → FAIL。
- [ ] **Step 3: 实现** `Conns`（`Clone`，内部 `Arc`）：
  - `senders: Arc<RwLock<HashMap<String, mpsc::Sender<ServerToAgent>>>>`
  - `pending: Arc<Mutex<HashMap<String, oneshot::Sender<CommandResult>>>>`（key=cmd_id）
  - `register(id) -> mpsc::Receiver<ServerToAgent>`（容量 32，存 sender，返回 receiver）
  - `send(id, msg) -> Result<()>`（无连接返回 Err）
  - `unregister(id)`
  - `new_pending(cmd_id) -> oneshot::Receiver<CommandResult>`
  - `resolve(cmd_id, result)`（取出并 send；无则忽略）
- [ ] **Step 4: 通过** `cargo test -p fleetwatch-server conn::`。
- [ ] **Step 5: Commit** `feat(server): per-connection sender + pending result table`

---

### Task 5: server — 接线 agent_ws（writer 任务 + summary 入库 + CommandResult 路由）

**Files:** Modify: `crates/server/src/agent_ws.rs`, `crates/server/src/store.rs`, `crates/server/src/db.rs`, `crates/server/src/lib.rs`

- [ ] **Step 1: db + store 快照（先 TDD store）**
  - db.rs SCHEMA 增：
    ```sql
    CREATE TABLE IF NOT EXISTS status_snapshots (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      machine_id TEXT NOT NULL,
      kind TEXT NOT NULL,
      json TEXT NOT NULL,
      captured_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_snap_machine ON status_snapshots(machine_id, captured_at);
    ```
  - store.rs 增并测试：
    ```rust
    pub async fn save_snapshot(pool,&str machine_id,&str kind,&str json)->Result<()>
    pub async fn latest_snapshot(pool,&str machine_id,&str kind)->Result<Option<(String,String)>> // (json, captured_at)
    pub async fn list_snapshots(pool,&str machine_id, limit:i64)->Result<Vec<Snapshot>>
    pub async fn purge_old_snapshots(pool, days:i64)->Result<u64> // 删除 captured_at < now-days
    ```
    测试 `snapshot_save_latest_purge`：存 2 条→latest 返回最新→把一条 captured_at 改成 40 天前→purge(30) 删 1 条。
- [ ] **Step 2: 运行失败 → 实现 → 通过**（store 测试）。Commit `feat(server): status_snapshots store + purge`.
- [ ] **Step 3: agent_ws 接线（集成测试驱动）**
  扩展 `crates/server/tests/online_flow.rs` 或新增 `tests/status_flow.rs`：agent 连上→server `dispatch(machine_id, RunStatus{kind:"host"})`→收到 `CommandResult`→`GET /api/machines/:id/status?kind=host` 返回含 hostname 的 JSON。
  实现：`agent_ws::run` 中
  - Hello 成功后：`let mut rx = st.conns.register(&machine_id).await;` 并 `tokio::select!` 同时处理「socket 读」与「rx 收到 ServerToAgent → ws.send」；
  - 收到 `Heartbeat{summary:Some(s)}` → `store::save_snapshot(pool, id, "summary", &serde_json::to_string(&s)?)`；
  - 收到 `CommandResult{cmd_id,..}` → `st.conns.resolve(&cmd_id, result)`；
  - 断开：`st.conns.unregister(&id)` + `registry.mark_offline`。
- [ ] **Step 4: 通过**（集成测试）。
- [ ] **Step 5: Commit** `feat(server): ws writer task, summary persistence, result routing`

---

### Task 6: server — dispatch + 状态 API

**Files:** Create: `crates/server/src/dispatch.rs`; Modify: `crates/server/src/api.rs`, `lib.rs`(路由)

- [ ] **Step 1: 失败测试**（dispatch 超时路径，单测）
```rust
#[tokio::test]
async fn dispatch_times_out_when_offline() {
    let conns = Conns::new();
    let err = dispatch(&conns, "ghost", ServerToAgent::Ping, std::time::Duration::from_millis(50)).await;
    assert!(err.is_err());
}
```
- [ ] **Step 2: 运行失败 → 实现**
  - `dispatch.rs`：
    ```rust
    pub async fn dispatch(conns:&Conns, machine_id:&str, msg:ServerToAgent, timeout:Duration)
        -> anyhow::Result<CommandResult>
    ```
    取 `cmd_id`（msg 内的）；`let rx = conns.new_pending(cmd_id).await;` → `conns.send(machine_id,msg).await?`（无连接即 Err）→ `tokio::time::timeout(timeout, rx).await??`。
  - api：`GET /api/machines/:id/status?kind=host`：生成 cmd_id（uuid）；`dispatch(&st.conns, id, RunStatus{cmd_id,kind,arg}, 30s)`；把返回的 `stdout`（JSON 字符串）解析回 JSON 并 `save_snapshot(id, kind, stdout)` 后返回；`GET /api/machines/:id/snapshots?limit=50` 返回历史。校验 kind 用 `fw_proto::messages::is_known_status_kind`，非法返回 400。
  - lib.rs 注册路由 + `mod dispatch;`；`AppState` 已含 `conns`。
- [ ] **Step 3: 通过**（dispatch 单测 + 第 5 任务的 status_flow 集成测试整体绿）。
- [ ] **Step 4: Commit** `feat(server): dispatch + on-demand status API`

---

### Task 7: server — 每日清理接线 + 收尾

**Files:** Modify: `crates/server/src/sweeper.rs`

- [ ] **Step 1: 实现** 在 sweeper 增一个每日 tick（`interval(Duration::from_secs(24*3600))`）调用 `store::purge_old_snapshots(&pool, 30)`；`spawn` 同时跑离线扫描与清理两个循环（或一个 select）。无需新测试（purge 已在 store 测过）。
- [ ] **Step 2: 验证** `cargo test --workspace` 全绿；`cargo build --workspace`。
- [ ] **Step 3: Commit** `feat(server): daily snapshot purge wired into sweeper`

---

## 自检（M2 spec 覆盖）
- 内置安全采集（host/cpu/mem/disk/net/proc/service）→ Task 2 ✅
- 心跳带轻量摘要并入库 → Task 3 + Task 5 ✅
- 按需拉详情（server 下发 RunStatus，agent 回 CommandResult，经 dispatch）→ Task 4/5/6 ✅
- 快照保留 1 个月 → Task 5(purge) + Task 7(每日触发) ✅
- kind 白名单防滥用 → Task 1 + Task 6 校验 ✅

## M2 完成标准
`cargo test --workspace` 全绿（新增 collectors / conn / dispatch / store snapshot 单测 + status_flow 集成测试）；server 能对在线 agent 拉到 host/cpu/mem/disk 详情并入库；摘要随心跳入库；超 30 天快照被清理。
