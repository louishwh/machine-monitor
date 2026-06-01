# 群哨（FleetWatch）· v1 设计文档

> 状态：设计已确认，待评审
> 日期：2026-05-30
> 代号：FleetWatch（群哨，最终名称待定）

---

## 1. 一句话定位

一个机器监控与远程查看平台：通过 `apt` / launchd 在每台机器上装一个轻量 agent，
agent 持**管理端签发的身份令牌**反向长连到中心服务端；操作者用一个 **PC 端桌面程序**
（唯一入口）随时查看各机器状态、按需下发命令。全栈 Rust。

## 2. 角色与组件

三个组件，职责单一、边界清晰：

1. **中心服务端 `fleetwatch-server`**（Rust/Axum，远程常驻）
   - 对 agent：WebSocket Hub，接受反向长连，维护在线连接池
   - 对操作者：控制面 API（仅供 PC 管理端调用，**无网页 UI、无匿名访问**）
   - **只验签、不签发**：用管理端公钥校验 agent 身份令牌；自身被攻破也签不出新机器
   - 命令分发 + 结果收集 + 状态/审计存储（SQLite）
2. **Agent `fleetwatch-agent`**（Rust 单静态二进制）
   - Ubuntu：`.deb` 安装，systemd 常驻
   - macOS：二进制 + launchd plist（后续可 Homebrew）
   - 持身份令牌反连 wss，定期上报基础状态，接命令→执行→回传
3. **PC 管理端 `fleetwatch-console`**（Tauri：Rust 核心 + React UI）
   - **信任根**：持主签名密钥，签发每台机器的身份令牌
   - **唯一入口**：是唯一能访问中心服务端控制面的客户端
   - 机器列表/在线状态、状态详情、命令控制台、签发/吊销机器、审计

```
[PC 管理端 console] ──签发机器身份──▶ 身份令牌 ──装机配置──▶ [Agent]
   │ 注册公钥(信任锚) / 控制面鉴权                              │ 持令牌反连 wss
   ▼                                                            ▼
[中心服务端 server] ◀──── 用 console 公钥验签，通过才接入 ◀──────┘
```

## 3. 信任与身份模型（核心）

- **PC 管理端 = 信任根**：首启生成一对 ed25519 主密钥；私钥存本机 OS 钥匙串，公钥注册到中心服务端作为信任锚。
- **机器身份令牌**：管理端为每台机器签发唯一令牌
  `base64(payload).base64(sig)`，`payload = { machineId, name, issuedAt }`，由管理端私钥签名。
- **provision（密钥来源）**：操作者把令牌配置到目标机器 agent —
  `fleetwatch-agent enroll --identity <令牌>` 或写入 `/etc/fleetwatch/agent.toml`。
  **身份密钥一律来源于 PC 管理端，无共享 bootstrap 口令。**
- **接入校验**：agent 反连时出示令牌 → 服务端用 console 公钥验签 + 查吊销列表 → 通过才接入。
- **吊销**：管理端吊销某机器 → 推吊销列表(machineId)到服务端 → 该身份立即失效。
- **console ↔ server 配对**：服务端首次部署设一个一次性 `pairing_token`；
  管理端首次连接用它注册自己的公钥。之后管理端通过**签名挑战**（证明持有主私钥）鉴权控制面，
  服务端仅信任这一个操作者身份（v1 单操作者）。

## 4. 连接协议（WS，JSON 帧）

Agent → Server：
- `Hello { identityToken, hostname, os, agentVersion }`
- `Heartbeat`
- `StatusReport { snapshot }`（周期上报基础状态）
- `CommandResult { cmdId, exit, stdout, stderr, done }`（可分块流式）

Server → Agent：
- `HelloAck { ok }` / `Reject { reason }`
- `RunStatus { cmdId, kind, arg? }`（内置采集器，如 service 名）
- `RunShell { cmdId, command }`（仅当该机 `shellEnabled=true`）
- `Ping`

心跳间隔 15s，45s 未心跳→标记离线。重连指数退避。传输 wss(rustls)。

## 5. 命令模型（两类）

**① 内置安全采集器**（随时可查，无需 shell 权限，agent 用 `sysinfo` + 读 /proc 实现）：
- 主机信息（os/内核/uptime/主机名）
- CPU 负载、内存、磁盘各挂载点用量、网络吞吐
- Top 进程（按 CPU/内存）
- 指定服务状态（Ubuntu：systemd active/failed；macOS：launchctl）

**② 受控任意命令**：仅当该机 `shellEnabled=true`（管理端逐机开启，默认关）；
服务端下发 `RunShell` → agent `sh -c` 带超时执行 → 回传；**每条全量审计**。

## 6. 数据模型（服务端 SQLite）

| 表 | 关键字段 |
|---|---|
| `machines` | id, name, hostname, os, agent_version, status(active/revoked), shell_enabled, last_seen, enrolled_at |
| `revocations` | machine_id, revoked_at |
| `status_snapshots` | machine_id, json, captured_at（留最新 + 小量历史） |
| `command_log` | id, machine_id, kind(status/shell), request, exit, output, created_at |
| `server_config` | console_public_key, pairing_token_hash, paired_at |

> 机器由管理端签发即可信，服务端不存"待审批"态——管理端签了就是授权。
> 服务端保留 `revocations` 以即时拒绝被吊销的身份。

## 7. PC 管理端（Tauri）界面

- 连接设置：填中心服务端地址 + 首次 pairing_token 配对；主密钥本机生成/存钥匙串
- 机器列表：在线/离线、名称、OS、最后心跳
- 签发机器：输入机器名 → 生成身份令牌（复制去装机）
- 机器详情：一键刷新状态、服务检查、Top 进程；开关 shell 权限；吊销
- 命令控制台：状态快捷查询永远可用；shell 输入仅在该机开权限时出现，带二次确认
- 审计日志

## 8. 打包与部署

- **Agent / Ubuntu**：`cargo-deb` 产 `fleetwatch-agent_<ver>_amd64.deb`，
  二进制到 `/usr/bin/`，systemd unit，配置 `/etc/fleetwatch/agent.toml`。
  装机：`apt install ./fleetwatch-agent.deb` → 填服务端地址 + 身份令牌 → `systemctl start`。
  后续可搭 apt 源做到 `apt-get install fleetwatch-agent`。
- **Agent / macOS**：二进制 + launchd plist（`~/Library/LaunchAgents` 或 `/Library/LaunchDaemons`）；
  `fleetwatch-agent enroll --identity <令牌>` 写配置后 `launchctl load`。后续 Homebrew tap。
- **服务端**：单二进制（可选 .deb），自带 SQLite，rustls TLS。
- **管理端**：Tauri 打 macOS `.app/.dmg`（操作者在 Mac 上用）。

## 9. 明确不做（v1 YAGNI）

告警通知、长期指标时序图、多操作者 RBAC、agent 自动升级、Windows agent、
机器分组标签、定时任务、OTA 自动 enroll（v1 手动 provision）、apt 在线源（v1 本地 .deb）。

## 10. 技术栈

| 部件 | 选型 |
|---|---|
| 中心服务端 | Rust、Axum、tokio、tokio-tungstenite(WS)、sqlx/SQLite、rustls、ed25519-dalek |
| Agent | Rust、tokio、tokio-tungstenite(client)、sysinfo、serde、toml |
| PC 管理端 | Tauri 2、Rust 核心、React + TS + Vite + Tailwind、ed25519-dalek、keyring |
| 共享协议 | `crates/proto`：WS 帧 + 身份令牌结构（server/agent 复用，console TS 镜像） |
| 打包 | cargo-deb（Ubuntu）、launchd plist（macOS）、Tauri bundler（管理端） |

## 11. 仓库结构（Cargo workspace 单仓）

```
monitor/
  Cargo.toml              workspace
  crates/
    proto/                共享协议与身份令牌类型
    server/               fleetwatch-server (bin)
    agent/                fleetwatch-agent (bin)
  console/                Tauri 管理端 (src-tauri + React)
  packaging/
    deb/                  systemd unit + 配置模板
    macos/                launchd plist
  docs/
```

## 12. 成功标准（v1）

- 在管理端签发身份 → Ubuntu/macOS 装 agent 配置令牌 → agent 反连上线、列表可见在线。
- 管理端能拉取目标机器的 CPU/内存/磁盘/进程/服务状态。
- 对开启 shell 权限的机器能下发任意命令并拿回输出，且全程审计。
- 吊销某机器后其连接立即被拒。
- 中心服务端无网页 UI、无匿名访问；唯一控制入口是 PC 管理端。
