# FleetWatch desktop console. Updated by scripts/update-homebrew-cask.sh.
# Install from this same repository:
#
#   brew tap louishwh/fleetwatch https://github.com/louishwh/machine-monitor
#   brew install --cask louishwh/fleetwatch/fleetwatch
cask "fleetwatch" do
  version "@VERSION@"
  sha256 "@SHA256@"

  url "https://github.com/louishwh/machine-monitor/releases/download/v#{version}/FleetWatch_#{version}_universal.dmg"
  name "FleetWatch"
  desc "Desktop manager for the FleetWatch machine fleet"
  homepage "https://github.com/louishwh/machine-monitor"

  depends_on :macos

  # Universal (arm64 + x86_64) build. Unless the release was signed and
  # notarized (APPLE_* secrets), Gatekeeper may ask for confirmation on
  # first launch: right-click the app in Finder and choose Open.
  app "FleetWatch.app"
end
