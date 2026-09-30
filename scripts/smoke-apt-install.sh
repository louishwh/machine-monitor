#!/bin/sh
# Run as root inside an Ubuntu container with the generated repository mounted
# at /apt-repo. Checks the same signed APT path a new Ubuntu host will use.
set -eu

repo="${1:-/apt-repo}"
arch="$(dpkg --print-architecture)"
install -d -m 0755 /etc/apt/keyrings
install -m 0644 "$repo/gpg.key" /etc/apt/keyrings/fleetwatch.asc
printf 'deb [arch=%s signed-by=/etc/apt/keyrings/fleetwatch.asc] file://%s stable main\n' \
    "$arch" "$repo" > /etc/apt/sources.list.d/fleetwatch.list

apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq fleetwatch-agent fleetwatch-server
dpkg-query -W -f='${Package} ${Status}\n' fleetwatch-agent fleetwatch-server
test "$(stat -c %a /etc/fleetwatch/agent.toml)" = 600
test "$(stat -c %a /etc/fleetwatch/server.toml)" = 640
/usr/bin/fleetwatch-agent --help >/dev/null
echo 'Signed APT install and config permissions: OK'
