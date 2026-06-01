#!/usr/bin/env bash
set -euo pipefail
# Minimal manual smoke validation for M1 — start server + agent and observe online status.
# Automated coverage is provided by the integration test in crates/server/tests/online_flow.rs.
echo "Fill in server.toml with the console_public_key_b64 that matches the signing key used to issue the token."
echo ""
echo "1) cargo run -p fleetwatch-server"
echo "2) fleetwatch-agent enroll --server ws://127.0.0.1:8080/agent --identity <token> --config /tmp/agent.toml"
echo "3) fleetwatch-agent run --config /tmp/agent.toml"
echo "4) curl http://127.0.0.1:8080/api/machines  # should show online:true"
