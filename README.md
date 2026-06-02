# FleetWatch（群哨）

机器监控与远程查看平台。通过 `apt` / launchd 在每台机器装一个轻量 **agent**，
agent 持 **管理端签发的身份令牌** 反向长连到中心 **server**；操作者用一个
**PC 管理端（Tauri 桌面）** 作为唯一入口，随时查看机器状态、下发命令。全栈 Rust。

```
[PC 管理端 console] ──签发机器身份──▶ 令牌 ──装机配置──▶ [Agent]
   │ 注册公钥(信任锚) / 签名鉴权调用                         │ 持令牌反连 wss
   ▼                                                         ▼
[中心服务端 server] ◀──── 用 console 公钥验签，通过才接入 ◀──┘
```

## 组件

- **`crates/proto`** — 共享协议：WS 消息、身份令牌（ed25519 签发/验签）、请求签名。
- **`crates/server`** — Axum 中心服务端：`/agent` WS Hub、`/api/*` 控制面（签名鉴权）、SQLite、在线注册表、命令分发、状态快照（保留 1 个月）、审计、离线扫描。
- **`crates/agent`** — 单二进制 agent：反连、心跳带状态摘要、按需采集（sysinfo）、受控 shell（30s 超时）。
- **`console/`** — Tauri 管理端：信任根主密钥（钥匙串）、配对、签发机器身份、机器列表/状态、命令控制台、审计、吊销。

## 安全模型

- 管理端是**信任根**：本机 ed25519 主密钥，私钥不出本机。
- 每台机器一把**唯一身份令牌**（管理端签发，可单独吊销）；server 只验签、不签发。
- 控制面仅认管理端（请求签名鉴权，无网页 UI、无匿名访问）。
- 受控 shell 默认关、逐机开、全量审计；吊销实时踢下线且重连被拒。

## 开发

```bash
cargo test --workspace          # proto + server + agent 全部测试
cargo build --workspace
cd console && pnpm install && pnpm app:dev   # 管理端开发模式
```

## 部署 / 打包

见 [`packaging/README.md`](packaging/README.md)：server(systemd)、agent(Ubuntu `.deb` / macOS launchd)、console(`tauri build`) 及端到端部署顺序。

## 设计与计划

- 设计：`docs/superpowers/specs/2026-05-30-fleetwatch-design.md`
- 计划：`docs/superpowers/plans/`（M1–M5）

## 里程碑

| | | 状态 |
|---|---|---|
| M1 | agent↔server 反连上线链路 | ✅ |
| M2 | 状态采集 + 按需拉取 + 快照保留 | ✅ |
| M3 | PC 管理端 + 配对 + 身份签发 + 控制面鉴权 | ✅ |
| M4 | 受控 shell + 审计 + 吊销 | ✅ |
| M5 | 打包（.deb / launchd / console bundle） | ✅ |

v1 之后：告警/通知 → 指标时序图表 → apt 在线源 → agent 自动升级。
