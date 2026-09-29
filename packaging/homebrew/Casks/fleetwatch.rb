# FleetWatch console cask.
#
# TEMPLATE — regenerated on every release by scripts/publish-tap.sh, which
# fills @VERSION@ / @SHA256@ / @URL@ and commits the result to the
# louishwh/homebrew-tap repo as Casks/fleetwatch.rb. Install:
#
#   brew install louishwh/tap/fleetwatch
cask "fleetwatch" do
  version "@VERSION@"
  sha256 "@SHA256@"

  url "@URL@"
  name "FleetWatch"
  desc "Desktop manager for the FleetWatch machine fleet"
  homepage "https://github.com/louishwh/machine-monitor"

  # Universal (arm64 + x86_64) build. Unless the release was signed and
  # notarized (APPLE_* secrets), Gatekeeper will ask for confirmation on
  # first launch: right-click → Open, or  xattr -cr /Applications/FleetWatch.app
  app "FleetWatch.app"
end
