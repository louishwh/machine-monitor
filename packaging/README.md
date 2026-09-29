# FleetWatch 打包与部署

## 在线安装（推荐）

配好 apt 源后可直接包管理器安装（README「Install」章节有完整命令）：

```bash
sudo apt update && sudo apt install fleetwatch-server   # 中心服务端
sudo apt update && sudo apt install fleetwatch-agent    # 每台被监控机器
brew install louishwh/tap/fleetwatch                    # macOS 桌面管理端
```

deb 由 tag 驱动的 release workflow 自动构建并发布到 gh-pages 上的签名 apt 源
（见 `RELEASING.md`）。下面是手动部署/本地构建的路径。

## 中心服务端（Ubuntu，单二进制 + systemd）

deb 安装时已自动创建 `fleetwatch` 系统用户、TLS 目录并 enable 服务；
手动部署时需自己做这些步骤：

```bash
cargo build --release -p fleetwatch-server
sudo install -m755 target/release/fleetwatch-server /usr/bin/
sudo useradd --system fleetwatch || true
sudo mkdir -p /etc/fleetwatch/tls /var/lib/fleetwatch
sudo chown -R fleetwatch /etc/fleetwatch/tls /var/lib/fleetwatch
# 写 /etc/fleetwatch/server.toml（模板：packaging/deb/server.toml.example）：
#   bind = "0.0.0.0:8443"          # 或 127.0.0.1:8080 由反代终止 TLS
#   db_path = "/var/lib/fleetwatch/fleetwatch.db"
#   pairing_token = "<一次性配对口令>"
#   console_public_key_b64 = ""    # 配对后由 /api/pair 写入 DB；此处可留空
sudo cp packaging/deb/fleetwatch-server.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable fleetwatch-server
```

反向代理（示例 Caddy）：把 `https://mon.example.com` 反代到 `127.0.0.1:8080`，
WebSocket 路径 `/agent` 需透传 Upgrade 头（Caddy 默认支持）。

## Agent —— Ubuntu（.deb）

在 Debian/Ubuntu 主机（或 Linux CI）上构建，musl 交叉构建用 zig：

```bash
make tools                      # 一次：zig + cargo-zigbuild + target
./scripts/build-deb.sh          # 本机 glibc 构建
./scripts/build-deb.sh x86_64-unknown-linux-musl      # 静态，任何 Ubuntu 可装
# 产出 target/[<triple>/]debian/fleetwatch-{agent,server}_<ver>_<arch>.deb
```

安装与启用：

```bash
sudo apt install ./target/debian/fleetwatch-agent_0.1.0_amd64.deb
# 安装后 service 已 enable 但未 start（需先填身份令牌）：
sudo fleetwatch-agent enroll \
    --server wss://mon.example.com/agent \
    --identity '<管理端签发的令牌>'
sudo systemctl start fleetwatch-agent
systemctl status fleetwatch-agent
```

## Agent —— macOS（LaunchDaemon）

```bash
cargo build --release -p fleetwatch-agent
sudo packaging/macos/install-macos.sh \
    target/release/fleetwatch-agent \
    wss://mon.example.com/agent \
    '<管理端签发的令牌>'
# 已 launchctl load，开机自启；日志 /var/log/fleetwatch-agent.log
```

## PC 管理端（Tauri，操作者的 Mac）

用户安装：`brew install louishwh/tap/fleetwatch`（release workflow 自动更新
cask；cask 模板在 `packaging/homebrew/Casks/fleetwatch.rb`）。

本地构建：

```bash
cd console
pnpm install
pnpm tauri build     # 产出 .app / .dmg（src-tauri/target/release/bundle/）
```

对外分发需 Apple 开发者证书签名 + 公证（CI 配置 `APPLE_*` secrets 后自动签名，
见 `RELEASING.md`）；本机自用可直接运行 `pnpm app:dev`。

## 端到端部署顺序

1. 起 server（设置 `pairing_token`）。
2. 管理端：生成主密钥 → 设服务端地址 → 用 `pairing_token` 配对（注册公钥）。
3. 管理端「签发机器」得到令牌。
4. 目标机装 agent（.deb / launchd）并 enroll 该令牌 → 启动。
5. 管理端机器列表看到在线、拉状态；按需开 shell、下命令、看审计、吊销。
