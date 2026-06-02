# FleetWatch M4 — 受控 shell + 审计 + 吊销 实现计划

> 用 superpowers:subagent-driven-development 执行。步骤用 `- [ ]`。

**Goal:** 在已签名鉴权的控制面上，新增受控任意命令（每机默认关、显式开启、30s 超时、全量审计）、命令审计日志查询、机器吊销（写吊销表 + 实时踢下线）。管理端提供 shell 开关、命令控制台、审计页。

**Architecture:** 复用 M2 的 `Conns`/`dispatch`（按 cmd_id 等结果）。`Conns` 增加每连接 kill 信号以支持吊销时实时断开。agent 复用 M2 的命令处理，新增 `RunShell` 分支（`sh -c`，30s 超时）。所有 shell 命令写 `command_log`。

**Tech Stack:** Rust/Axum、tokio(process/timeout/oneshot)、sqlx；console React。

---

## M4a — 服务端

### Task 1: store — command_log + shell_enabled + 吊销

**Files:** Modify `crates/server/src/db.rs`, `crates/server/src/store.rs`

- [ ] **Step 1** db.rs SCHEMA 增：
```sql
CREATE TABLE IF NOT EXISTS command_log (
  id TEXT PRIMARY KEY,
  machine_id TEXT NOT NULL,
  kind TEXT NOT NULL,          -- 'shell' | 'status'
  request TEXT NOT NULL,       -- 命令文本 或 status kind
  exit INTEGER,
  output TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_audit_machine ON command_log(machine_id, created_at);
```
- [ ] **Step 2 TDD store**（用 in-memory pool）：
  - `set_shell_enabled(pool,&str id,bool)->Result<()>`、`get_shell_enabled(pool,&str id)->Result<bool>`
  - `log_command(pool, &CommandLogEntry)->Result<()>`（结构含 id/machine_id/kind/request/exit:Option<i64>/output/created_at）
  - `list_audit(pool, machine_id:Option<&str>, limit:i64)->Result<Vec<AuditRow>>`（按时间倒序）
  - `add_revocation(pool,&str id)->Result<()>`（INSERT OR IGNORE 进 revocations）
  测试 `shell_toggle_and_audit`：默认 shell_enabled=false→set true→get true；log 两条→list_audit 返回 2 条且最新在前；add_revocation→is_revoked true。
- [ ] **Step 3 通过 + Commit** `feat(server): command_log + shell toggle + revocation store`

---

### Task 2: conns — kill 信号（吊销实时断开）

**Files:** Modify `crates/server/src/conn.rs`, `crates/server/src/agent_ws.rs`

- [ ] **Step 1 TDD conn**：`Conns` 每连接增一个 `kill: oneshot`（或 `tokio::sync::Notify`/`watch`）。新增：
  - `register` 返回 `(mpsc::Receiver<ServerToAgent>, KillReceiver)`（或返回一个含两者的句柄）
  - `kick(id)`：触发该连接 kill；移除 sender
  测试 `kick_signals`：register→kick→kill receiver 被触发（`recv()`/`notified()` 返回）。
- [ ] **Step 2** `agent_ws::run`：把主循环改为 `tokio::select!` 三路：socket 读 / `rx` 收到下行消息→`ws.send` / `kill` 触发→发送 `Reject("已吊销")` 并 `break`（随后正常 unregister + mark_offline）。
- [ ] **Step 3 通过**（conn 单测 + 现有集成测试仍绿）**Commit** `feat(server): per-connection kill for live revoke`

---

### Task 3: api — shell 开关 / run-shell / revoke / audit

**Files:** Create `crates/server/src/admin.rs`（或扩 `api.rs`）; Modify `lib.rs`（受签名鉴权保护的路由组）

全部挂在 **签名鉴权中间件** 后（与 /api/machines 同组）：
- `PATCH /api/machines/:id/shell`  body `{enabled:bool}` → `store::set_shell_enabled`；200。
- `POST  /api/machines/:id/run-shell` body `{command:String}`：
  - `get_shell_enabled` 为 false → 403。
  - 生成 cmd_id；`dispatch(&conns, id, RunShell{cmd_id, command}, 30s)`（离线/超时 → 502/504）。
  - `log_command(kind="shell", request=command, exit, output=stdout+stderr)`。
  - 返回 `{exit, stdout, stderr}`。
- `POST  /api/machines/:id/revoke` → `store::add_revocation` + `conns.kick(id)` + `registry.mark_offline`；200。
- `GET   /api/audit?machine_id=&limit=` → `store::list_audit`。
- [ ] **Step 1 集成测试** `crates/server/tests/shell_flow.rs`：测试 agent 连上（带一个会响应 RunShell 的最小逻辑——见 Task 4，集成测试可直接用真 agent 客户端逻辑或一个内联 fake：发 Hello 后，对收到的 RunShell 回 `CommandResult{exit:0,stdout:"hi",...}`）：
  - shell 未开 → run-shell 返回 403。
  - 开启 shell → run-shell 返回 stdout "hi"；`GET /api/audit` 含该条。
  - revoke → 该连接被踢（ws 收到 Reject / 关闭）；之后用同令牌重连被拒（M1 行为）。
  所有控制面请求带签名头（复用 M3 的 signed_get helper）。
- [ ] **Step 2 实现 → 通过 + Commit** `feat(server): shell toggle, run-shell, revoke, audit API`

---

### Task 4: agent — 执行 RunShell（30s 超时）

**Files:** Modify `crates/agent/src/client.rs`

- [ ] **Step 1 TDD**（纯函数/可注入）：新增 `async fn run_shell(command:&str, timeout:Duration) -> (i32,String,String)`：用 `tokio::process::Command::new("sh").arg("-c").arg(command)` 采集 stdout/stderr/exit；`tokio::time::timeout` 超时则 kill 返回 exit=-1、stderr="timeout"。测试 `run_shell_echo`（`echo hi` → exit 0, stdout 含 "hi"）、`run_shell_timeout`（`sleep 5` + 100ms 超时 → exit -1）。
- [ ] **Step 2** `connect_once` 处理 `ServerToAgent::RunShell{cmd_id,command}` → `run_shell(command, 30s)` → 回 `CommandResult{cmd_id,exit,stdout,stderr,done:true}`（同 RunStatus 一样走 spawn 安全路径，避免阻塞）。
- [ ] **Step 3 通过 + Commit** `feat(agent): execute RunShell with 30s timeout`

---

## M4b — 管理端 UI

### Task 5: console — shell 开关 + 命令控制台 + 吊销 + 审计

**Files:** Modify `console/src-tauri/src/{api_client.rs,commands.rs,lib.rs}`、`console/src/...`

- [ ] **Step 1** Rust 命令封装：`signed_post(server, path, body, sk)`（带签名，body 参与 canonical）；Tauri 命令 `set_shell(id,enabled)`、`run_shell(id,command)->{exit,stdout,stderr}`、`revoke_machine(id)`、`audit(machineId?)->Value`。注意 **POST 的签名 body 必须与发送 body 字节一致**（先序列化为字符串，既用于签名也用于发送）。Commit `feat(console): signed POST + shell/revoke/audit commands`。
- [ ] **Step 2** UI：机器详情里加「shell 权限」开关（调 set_shell）；当开启时显示命令输入框 + 执行按钮（run_shell，带二次确认）→ 显示 exit/stdout/stderr；「吊销」按钮（confirm）；新增「审计」页或详情内审计列表（audit）。Commit `feat(console-ui): command console, shell toggle, revoke, audit`。

---

## 自检（M4 spec 覆盖）
- 受控任意命令、默认关、逐机开 → Task 1/3/5 ✅
- 30s 超时 → Task 4 ✅
- 全量审计（命令+输出入库、可查）→ Task 1/3/5 ✅
- 吊销 + 实时踢下线 + 重连被拒 → Task 2/3 ✅
- 全部经签名鉴权控制面（POST body 纳入签名）→ Task 3/5 ✅

## M4 完成标准
`cargo test -p fleetwatch-server -p fleetwatch-agent` 全绿（新增 shell_flow 集成 + run_shell 单测 + store 单测）；console `tsc`+`vite build`+`cargo build` 通过；端到端：开启某机 shell→管理端下发命令拿到输出→审计可见→吊销后该机被踢且重连被拒。
