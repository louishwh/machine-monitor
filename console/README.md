# FleetWatch 管理端

Tauri 2 桌面应用 — FleetWatch 的信任根与唯一控制入口。

## 架构概述

```
OS Keychain
  └─ master_seed (ed25519 私钥)
       ├─ 生成公钥 → 配对服务端（注册公钥）
       ├─ 签名控制面 HTTP 请求（x-fw-timestamp / x-fw-signature）
       └─ 签发机器身份令牌 → 运维在目标机器上运行 agent
```

私钥永不离开本机钥匙串。

---

## 前端 (Vite + React + TypeScript + Tailwind)

### 依赖

```bash
pnpm install
```

### 开发模式（纯前端，不启动 Tauri）

```bash
pnpm dev
# → http://localhost:1430
```

### 构建前端 dist

```bash
pnpm build
# 或
./node_modules/.bin/vite build
```

### 类型检查

```bash
./node_modules/.bin/tsc
```

---

## Tauri 应用

### 开发模式（前端 + Tauri shell）

```bash
pnpm app:dev
```

### 打包发布

```bash
pnpm app:build
# 产物在 src-tauri/target/release/bundle/
```

### 仅编译 Rust 部分

```bash
cd src-tauri && cargo build
```

---

## 运营商工作流

下面是从零到一台机器在线的完整流程：

### 1. 生成主密钥

打开管理端 → 连接 & 配对 → **生成主密钥**。

公钥将显示在界面上，私钥存储在 macOS/Linux 钥匙串，不可导出。

### 2. 配置服务端地址

在「服务端地址」栏填入服务端的 HTTP 地址，例如：

```
http://192.168.1.10:3000
```

点击**保存**。

### 3. 配对服务端

服务端启动配置（`server.toml` 或环境变量 `PAIRING_TOKEN`）中有一个一次性配对码。

在「配对码」栏输入该码，点击**配对**。成功后服务端注册本控制台的公钥，后续所有控制面请求由主私钥签名鉴权。

> 配对只能进行一次。若需重置，在服务端清空 `server_config` 表后重新配对。

### 4. 签发机器身份令牌

切换到「机器」视图 → **签发机器** → 输入机器名称（如 `prod-server-01`）→ 点击**签发身份令牌**。

界面会显示令牌和完整的注册命令，例如：

```bash
fleetwatch-agent enroll \
  --server http://192.168.1.10:3000 \
  --identity eyJ...（完整令牌）
```

点击「复制」按钮将命令复制到剪贴板。

### 5. 在目标机器上运行 Agent

将第 4 步复制的命令在目标机器上执行：

```bash
fleetwatch-agent enroll --server <url> --identity <token>
fleetwatch-agent start
```

Agent 启动后通过 WebSocket 连接服务端，服务端用管理端的公钥验证身份令牌。

### 6. 在管理端查看机器在线状态

回到「机器」视图 — 列表每 5 秒自动刷新，正常情况下目标机器变为「在线」。

点击机器行可进入详情：

- **主机 / CPU / 内存 / 磁盘 / 网络 / 进程 / 服务** — 按需拉取当前状态
- **最近快照** — 历史快照列表

---

## 项目结构

```
console/
├── src/                     React 前端
│   ├── api.ts               Tauri invoke 类型化包装
│   ├── ui.tsx               Toast / Spinner / Modal 组件
│   ├── App.tsx              应用 Shell（侧边栏导航）
│   └── pages/
│       ├── ConnectPage.tsx  连接 & 配对页
│       └── MachinesPage.tsx 机器列表 / 签发 / 详情
├── src-tauri/               Tauri / Rust 核心
│   └── src/
│       ├── master_key.rs    ed25519 主密钥（keyring）
│       ├── settings.rs      服务端地址（keyring）
│       ├── api_client.rs    签名 HTTP 客户端
│       └── commands.rs      Tauri IPC 命令
└── README.md
```
