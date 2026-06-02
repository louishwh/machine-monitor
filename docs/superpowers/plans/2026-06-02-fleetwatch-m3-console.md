# FleetWatch M3 — PC 管理端 + 身份签发 + 配对 实现计划

> 用 superpowers:subagent-driven-development 执行。步骤用 `- [ ]`。

**Goal:** 把 PC 管理端（Tauri 桌面）做成信任根与唯一控制入口：本机生成 ed25519 主密钥，与中心服务端一次性配对（注册公钥），签发机器身份令牌，并通过签名鉴权的控制面查看机器列表/在线/状态。服务端控制面从此只认这个管理端。

**Architecture:** 服务端新增 `server_config`（存 console 公钥 + 配对状态），agent 身份验签的公钥来源从静态配置改为 DB（配对后写入）。控制面 `/api/*`（除 `/api/pair`）加签名鉴权中间件（管理端用主私钥对 `method|path|timestamp|body-hash` 签名）。Tauri 管理端：keyring 存主私钥，配对、签发、签名调用。

**Tech Stack:** Rust/Axum、ed25519-dalek、sqlx；Tauri 2 + React/TS/Tailwind + keyring（复用 content-machine 模式）。

---

## 阶段与文件结构

### M3a — 服务端：配对 + 控制面鉴权（先做，PC 端依赖它）
```
crates/server/src/db.rs          (改) server_config 表
crates/server/src/store.rs       (改) get/set console_pubkey + pairing 状态
crates/server/src/lib.rs         (改) AppState 持 Arc<RwLock<Option<Vec<u8>>>> console_pubkey；启动时从 DB 载入；路由分组
crates/server/src/agent_ws.rs    (改) 验签公钥改读 AppState 的共享 console_pubkey（None 时拒绝）
crates/server/src/pair.rs        (新) POST /api/pair：一次性 pairing_token → 写公钥
crates/server/src/auth.rs        (新) 控制面签名鉴权中间件 + 签名校验
crates/server/src/api.rs         (改) machines/status/snapshots 路由套上鉴权
crates/server/tests/*            (改) 集成测试带签名头；新增 pair_flow 测试
crates/proto/src/auth.rs         (新) 规范化签名串 + sign/verify 请求签名（server/console 共用）
```

### M3b — 管理端 Rust 核心（Tauri）
```
console/                          (新) Tauri 应用
  src-tauri/Cargo.toml, tauri.conf.json, build.rs, src/main.rs, src/lib.rs
  src-tauri/src/master_key.rs     主密钥生成/钥匙串存储（keyring）
  src-tauri/src/api_client.rs     签名 HTTP 客户端（reqwest，复用 proto::auth）
  src-tauri/src/commands.rs       Tauri IPC：master_status/pair/issue_machine/list_machines/get_status/...
```

### M3c — 管理端 UI（React）
```
console/src/                      连接&配对页 / 机器列表(在线) / 签发机器 / 机器详情(状态)
```

---

## M3a — 服务端

### Task 1: proto — 请求签名工具

**Files:** Create `crates/proto/src/auth.rs`; Modify `crates/proto/src/lib.rs`(`pub mod auth;`)

- [ ] **Step 1 失败测试**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    #[test]
    fn sign_verify_request() {
        let sk = SigningKey::from_bytes(&[5u8;32]);
        let vk = sk.verifying_key();
        let ts = "2026-06-02T00:00:00Z";
        let sig = sign_request(&sk, "GET", "/api/machines", ts, b"");
        assert!(verify_request(vk.as_bytes(), "GET", "/api/machines", ts, b"", &sig).is_ok());
        assert!(verify_request(vk.as_bytes(), "POST", "/api/machines", ts, b"", &sig).is_err());
    }
}
```
- [ ] **Step 2 失败 → 实现**
```rust
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};   // add sha2 = "0.10" to proto deps

fn canonical(method:&str, path:&str, ts:&str, body:&[u8]) -> Vec<u8> {
    let mut h = Sha256::new(); h.update(body);
    let body_hex = format!("{:x}", h.finalize());
    format!("{method}\n{path}\n{ts}\n{body_hex}").into_bytes()
}
pub fn sign_request(sk:&SigningKey, method:&str, path:&str, ts:&str, body:&[u8]) -> String {
    B64.encode(sk.sign(&canonical(method,path,ts,body)).to_bytes())
}
pub fn verify_request(pubkey:&[u8], method:&str, path:&str, ts:&str, body:&[u8], sig_b64:&str)
    -> Result<(), String> {
    let key:[u8;32] = pubkey.try_into().map_err(|_| "bad key".to_string())?;
    let vk = VerifyingKey::from_bytes(&key).map_err(|e| e.to_string())?;
    let sig_bytes = B64.decode(sig_b64).map_err(|e| e.to_string())?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|e| e.to_string())?;
    vk.verify_strict(&canonical(method,path,ts,body), &sig).map_err(|_| "sig".to_string())
}
```
- [ ] **Step 3 通过** `cargo test -p fw-proto auth::`. **Commit** `feat(proto): request signing (canonical method|path|ts|bodyhash)`

---

### Task 2: server — server_config 表 + store

**Files:** Modify `db.rs`, `store.rs`

- [ ] **Step 1** db.rs SCHEMA 增：
```sql
CREATE TABLE IF NOT EXISTS server_config (
  id INTEGER PRIMARY KEY CHECK (id=1),
  console_public_key TEXT,
  paired_at TEXT
);
INSERT OR IGNORE INTO server_config (id) VALUES (1);
```
（最后一句单独 execute，或在 run_migrations 里 INSERT OR IGNORE。）
- [ ] **Step 2 store TDD**：`get_console_pubkey(pool)->Result<Option<String>>`、`set_console_pubkey(pool,&str)->Result<()>`（写 b64 + paired_at=now）、`is_paired(pool)->Result<bool>`。测试 `pairing_state`：初始未配对→set→is_paired true + get 返回值。
- [ ] **Step 3 通过 + Commit** `feat(server): server_config store (console pubkey/paired)`

---

### Task 3: server — AppState 共享公钥 + agent_ws 改用

**Files:** Modify `lib.rs`, `agent_ws.rs`, `crates/server/tests/online_flow.rs`, `status_flow.rs`

- [ ] **Step 1** `AppState` 把 `console_pubkey: Arc<Vec<u8>>` 改为 `console_pubkey: Arc<RwLock<Option<Vec<u8>>>>`。`run()` 启动时从 `store::get_console_pubkey` 载入（有则 Some）。`spawn_test_server(pubkey_b64)` 直接预置为 `Some(decoded)`（保持 M1/M2 集成测试行为：已配对态）。
- [ ] **Step 2** `agent_ws`：取 `let guard = st.console_pubkey.read().await; let Some(pk)=guard.as_ref() else { Reject("未配对"); return };` 再 `verify_identity(pk, token)`。
- [ ] **Step 3** 现有集成测试：`spawn_test_server` 仍预置公钥，故 online_flow/status_flow 不需改鉴权（agent 面不加签名）；但**控制面**调用要带签名（见 Task 5）——本任务先确保 agent 面回归绿。
- [ ] **Step 4 通过**（`cargo test -p fleetwatch-server`）**Commit** `refactor(server): DB-backed console pubkey via shared RwLock`

---

### Task 4: server — /api/pair

**Files:** Create `pair.rs`; Modify `lib.rs`(路由 + ServerConfig 增 `pairing_token`)

- [ ] **Step 1** `ServerConfig` 增 `pairing_token: String`。`pair.rs`：
```
POST /api/pair  body { pairing_token, console_public_key_b64 }
- if store::is_paired → 409 已配对
- if body.pairing_token != cfg.pairing_token → 401
- 校验 pubkey 32 字节；store::set_console_pubkey；同时更新 AppState 共享公钥；200 {ok:true}
```
`AppState` 需能读 `pairing_token`（把 `pairing_token: Arc<String>` 放进 AppState，或整个 `Arc<ServerConfig>`）。
- [ ] **Step 2 集成测试** `pair_flow.rs`：未配对的 server（spawn_test_server 改造出一个「未预置公钥」变体 `spawn_unpaired(pairing_token)`）→ POST /api/pair 正确 token+pubkey → 200；再 POST → 409；错误 token → 401。
- [ ] **Step 3 通过 + Commit** `feat(server): /api/pair one-time console pairing`

---

### Task 5: server — 控制面签名鉴权中间件

**Files:** Create `auth.rs`; Modify `lib.rs`(给 `/api/machines*` 套 middleware), `api.rs`，更新 `online_flow.rs`/`status_flow.rs` 带签名头

- [ ] **Step 1** `auth.rs`：axum middleware `require_console_sig`：
  - 读 header `x-fw-timestamp`、`x-fw-signature`；缺失 → 401
  - 时间戳偏差 > 300s → 401
  - 取 AppState 共享 console_pubkey（None → 401 未配对）
  - 读 method/path、body bytes（用 `axum::body::to_bytes` 后重建 request）；`fw_proto::auth::verify_request` → 失败 401
  - 通过则放行
  路由：`/api/pair` 不加；`/api/machines`、`/api/machines/:id/status`、`/snapshots` 加 `route_layer(middleware::from_fn_with_state(state, require_console_sig))`。
- [ ] **Step 2** 更新 `online_flow.rs` / `status_flow.rs`：调用控制面前用测试私钥签名并带上 `x-fw-timestamp`/`x-fw-signature` 头（测试已持 SigningKey）。提供一个测试 helper `signed_get(addr, path, &sk)`。
- [ ] **Step 3 集成测试** 新增 `auth_flow.rs`：无签名 GET /api/machines → 401；错误签名 → 401；正确签名 → 200。
- [ ] **Step 4 通过**（全 server 测试绿）**Commit** `feat(server): control-plane signature auth middleware`

---

## M3b — 管理端 Rust 核心（Tauri）

### Task 6: Tauri 脚手架（console/）
参照 content-machine 的 Tauri 2 结构搭 `console/`：`package.json`（含 `pnpm.onlyBuiltDependencies:["esbuild"]`、`.npmrc verify-deps-before-run=false`）、Vite+React+TS+Tailwind、`src-tauri`（Cargo.toml 依赖：tauri 2、tokio、reqwest(rustls)、ed25519-dalek、base64、keyring、serde、serde_json、fw-proto via path `../../crates/proto`、chrono、sha2）、tauri.conf.json、图标（用 tauri icon 生成）。`cargo build` + 前端 `vite build` 通过。**Commit** `chore(console): tauri scaffold`。
> 注意：console 的 src-tauri 是**独立 crate**（不在 monitor 的 workspace members 里，避免 Tauri 与服务端 workspace 冲突）——通过 path 依赖引用 `fw-proto`。其 `Cargo.toml` 不写 `[workspace]`，或显式 `[workspace]` 空表使其自成一体。

### Task 7: master_key.rs（主密钥）
TDD（纯逻辑可测：生成→公钥 b64 稳定；签名验签）。`generate_or_load() -> SigningKey`（首次生成 32 字节 seed，存 keyring service `com.fleetwatch.console` user `master_seed`；存在则载入）。`public_key_b64()`。`has_key()`。Tauri 命令 `master_status() -> {hasKey, publicKeyB64}`、`generate_master_key()`. **Commit** `feat(console): master key (keyring-backed ed25519)`。

### Task 8: api_client.rs + commands.rs（配对/签发/查询）
- `api_client`：`pair(server,pairing_token)`（POST /api/pair 带 console pubkey）；`signed_get(server, path, &sk) -> Value`（用 `fw_proto::auth::sign_request` 加 `x-fw-timestamp`/`x-fw-signature`）。
- `issue_machine(name) -> token`：`fw_proto::token::sign_identity(master, IdentityPayload{machine_id:uuid, name, issued_at:now})`，返回令牌串（供运维装机）。
- Tauri 命令：`pair_server(server,token)`、`issue_machine(server?,name)->{machineId,token}`、`list_machines(server)->Value`、`machine_status(server,id,kind)->Value`、`machine_snapshots(server,id)`。
- 服务端地址存本地（tauri store 或一个 settings 文件/keyring）。
TDD 对纯函数（令牌签发可被 server 的 verify_identity 接受——可在 console crate 写一个用 fw-proto 验证的单测）。**Commit** `feat(console): pairing + identity issuance + signed api client`。

---

## M3c — 管理端 UI（React）

### Task 9: 连接 & 配对页
设置中心服务端地址；显示主密钥状态（无则「生成主密钥」）；输入 pairing_token「配对」；配对成功提示。**Commit** `feat(console-ui): connect & pair page`。

### Task 10: 机器列表 + 签发 + 详情
- 机器列表：调 `list_machines`，显示在线/离线、名称、OS、最后心跳；轮询刷新（如 5s）。
- 「签发机器」弹窗：输机器名 → 调 `issue_machine` → 显示令牌 + 复制按钮（运维拿去 `fleetwatch-agent enroll --identity <token>`）。
- 机器详情：按钮拉 host/cpu/mem/disk 状态（`machine_status`），展示 JSON/卡片；最近快照列表。
**Commit** `feat(console-ui): machines list, issue machine, status detail`。

### Task 11: 收尾
`console` 前端 `vite build` + `cargo build`（debug）通过；写 `console/README.md`（开发/打包/配对/签发说明）。可选 `tauri build` 出 .app（耗时，按需）。**Commit** `docs(console): readme + build`。

---

## 自检（M3 spec 覆盖）
- 管理端=信任根（本机主密钥，私钥不出本机）→ Task 7 ✅
- 与 server 一次性配对、注册公钥 → Task 4 + Task 8/9 ✅
- 签发机器身份令牌（server 验签接受）→ Task 8 + 复用 proto::token ✅
- 控制面仅认管理端（签名鉴权，无网页/匿名）→ Task 5 ✅
- 机器列表/在线/状态查看 → Task 10 ✅
- agent 验签公钥来源改为配对后的 DB 公钥 → Task 3 ✅

## M3 完成标准
`cargo test -p fw-proto -p fleetwatch-server`（含 pair_flow/auth_flow）全绿；console `cargo build` + `vite build` 通过；端到端：管理端生成主密钥→配对 server→签发机器令牌→agent 用该令牌上线→管理端列表看到在线并能拉到状态；未签名的控制面请求被 401。
