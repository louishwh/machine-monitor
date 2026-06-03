# FleetWatch — build & packaging
# All packaging for the server/agent (Linux) and the console app (macOS) lives here.
#
# Quick start:
#   make tools          # one-time: cross-compile toolchain
#   make dist           # build everything (linux binaries + fwctl + app)
#   make help           # list all targets

SHELL := /bin/bash
.DEFAULT_GOAL := help

# Linux target for server/agent: static musl, runs on any x86_64 Linux (22.04/24.04…)
LINUX_TARGET ?= x86_64-unknown-linux-musl
LINUX_DIR    := target/$(LINUX_TARGET)/release
CONSOLE      := console
TAURI        := ./node_modules/.bin/tauri
VERSION      := $(shell grep -m1 '^version' crates/server/Cargo.toml | cut -d'"' -f2)

# Deploy params (override on the command line):
#   make deploy-server HOST=root@hz
#   make deploy-agent  HOST=ubuntu@sg2c4g SUDO=sudo
HOST ?=
SUDO ?=
SSH  ?= ssh

.PHONY: help
help: ## List targets
	@echo "FleetWatch make targets (version $(VERSION)):"
	@grep -hE '^[a-zA-Z0-9_-]+:.*##' $(MAKEFILE_LIST) \
	  | sort | awk 'BEGIN{FS=":.*## "}{printf "  \033[36m%-16s\033[0m %s\n",$$1,$$2}'

# ── prerequisites ─────────────────────────────────────────────────────────────
.PHONY: tools
tools: ## One-time: install cross-compile toolchain (zig + cargo-zigbuild + targets)
	@command -v zig >/dev/null || { echo "Install zig first:  brew install zig"; exit 1; }
	rustup target add $(LINUX_TARGET)
	@command -v cargo-zigbuild >/dev/null || cargo install cargo-zigbuild

# ── tests ─────────────────────────────────────────────────────────────────────
.PHONY: test
test: ## Run the Rust workspace tests
	cargo test --workspace

# ── Linux binaries (static musl, x86_64) ──────────────────────────────────────
.PHONY: build-server
build-server: ## Cross-compile the server -> static linux x86_64
	cargo zigbuild --release --target $(LINUX_TARGET) -p fleetwatch-server
	@echo "==> $(LINUX_DIR)/fleetwatch-server"

.PHONY: build-agent
build-agent: ## Cross-compile the agent -> static linux x86_64
	cargo zigbuild --release --target $(LINUX_TARGET) -p fleetwatch-agent
	@echo "==> $(LINUX_DIR)/fleetwatch-agent"

.PHONY: build-linux
build-linux: build-server build-agent ## Build both server + agent linux binaries

# ── fwctl (operator CLI, host-native) ─────────────────────────────────────────
.PHONY: build-fwctl
build-fwctl: ## Build fwctl (operator console CLI) for this host
	cargo build --release -p fwctl
	@echo "==> target/release/fwctl"

# ── agent .deb (Debian/Ubuntu; run on Linux or CI) ────────────────────────────
.PHONY: deb-agent
deb-agent: ## Build the agent .deb (needs cargo-deb; run on Linux/CI)
	@command -v cargo-deb >/dev/null || cargo install cargo-deb
	cargo build --release -p fleetwatch-agent
	cargo deb -p fleetwatch-agent --no-build
	@echo "==> target/debian/*.deb"

# ── console app (Tauri, macOS) ────────────────────────────────────────────────
.PHONY: app-deps
app-deps: ## Install console frontend deps
	cd $(CONSOLE) && pnpm install && (pnpm rebuild esbuild >/dev/null 2>&1 || true)

.PHONY: app
app: app-deps ## Build the console app (.app/.dmg for this arch)
	cd $(CONSOLE) && $(TAURI) build
	@echo "==> $(CONSOLE)/src-tauri/target/release/bundle/"

.PHONY: app-universal
app-universal: app-deps ## Build the console app as a universal (arm64+x86_64) bundle
	rustup target add x86_64-apple-darwin aarch64-apple-darwin
	cd $(CONSOLE) && $(TAURI) build --target universal-apple-darwin
	@echo "==> $(CONSOLE)/src-tauri/target/universal-apple-darwin/release/bundle/"

# ── distributable bundle (linux tarball: binaries + systemd/launchd + docs) ────
.PHONY: pkg-linux
pkg-linux: build-linux ## Tar up linux binaries + packaging files for shipping
	@rm -rf dist && mkdir -p dist/fleetwatch-$(VERSION)/{bin,packaging}
	cp $(LINUX_DIR)/fleetwatch-server $(LINUX_DIR)/fleetwatch-agent dist/fleetwatch-$(VERSION)/bin/
	cp -r packaging/* dist/fleetwatch-$(VERSION)/packaging/
	cp README.md dist/fleetwatch-$(VERSION)/ 2>/dev/null || true
	cd dist && tar czf fleetwatch-$(VERSION)-linux-x86_64.tar.gz fleetwatch-$(VERSION)
	@echo "==> dist/fleetwatch-$(VERSION)-linux-x86_64.tar.gz"

# ── everything ────────────────────────────────────────────────────────────────
.PHONY: dist
dist: build-linux build-fwctl app ## Build everything: linux binaries + fwctl + app

# ── deploy (parameterized; restarts the systemd service) ──────────────────────
.PHONY: deploy-server
deploy-server: build-server ## Deploy+restart server (make deploy-server HOST=root@hz)
	@test -n "$(HOST)" || { echo "set HOST, e.g. HOST=root@hz"; exit 1; }
	scp $(LINUX_DIR)/fleetwatch-server $(HOST):/usr/local/bin/fleetwatch-server.new
	$(SSH) $(HOST) 'mv /usr/local/bin/fleetwatch-server.new /usr/local/bin/fleetwatch-server \
	  && chmod +x /usr/local/bin/fleetwatch-server && systemctl restart fleetwatch-server \
	  && systemctl is-active fleetwatch-server'

.PHONY: deploy-agent
deploy-agent: build-agent ## Deploy+restart agent (make deploy-agent HOST=ubuntu@sg2c4g SUDO=sudo)
	@test -n "$(HOST)" || { echo "set HOST, e.g. HOST=ubuntu@sg2c4g SUDO=sudo"; exit 1; }
	scp $(LINUX_DIR)/fleetwatch-agent $(HOST):/tmp/fleetwatch-agent
	$(SSH) $(HOST) '$(SUDO) install -m755 /tmp/fleetwatch-agent /usr/local/bin/fleetwatch-agent \
	  && $(SUDO) systemctl restart fleetwatch-agent && $(SUDO) systemctl is-active fleetwatch-agent'

# ── clean ─────────────────────────────────────────────────────────────────────
.PHONY: clean
clean: ## Remove build artifacts (workspace + console + dist)
	cargo clean
	rm -rf dist $(CONSOLE)/dist $(CONSOLE)/src-tauri/target
