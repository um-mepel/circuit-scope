cask "circuit-scope" do
  version "0.2.2"

  # SHA-256 checksums for the DMG attached to the GitHub Release.
  # Regenerate after each release with:
  #   shasum -a 256 "Circuit Scope_<version>_aarch64.dmg"
  #   shasum -a 256 "Circuit Scope_<version>_x64.dmg"
  # Tauri's DMG bundle includes the product name with a space ("Circuit Scope_<ver>_<arch>.dmg");
  # GitHub serves that asset URL with the space percent-encoded as %20.
  on_arm do
    sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    url "https://github.com/um-mepel/circuit-scope/releases/download/v#{version}/Circuit%20Scope_#{version}_aarch64.dmg",
        verified: "github.com/um-mepel/circuit-scope/"
  end
  on_intel do
    sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    url "https://github.com/um-mepel/circuit-scope/releases/download/v#{version}/Circuit%20Scope_#{version}_x64.dmg",
        verified: "github.com/um-mepel/circuit-scope/"
  end

  name "Circuit Scope"
  desc "Verilog (IEEE 1364) IDE: edit, simulate to VCD, waveform viewer"
  homepage "https://github.com/um-mepel/circuit-scope"

  livecheck do
    url :url
    strategy :github_latest
  end

  auto_updates false
  depends_on macos: ">= :big_sur"

  app "Circuit Scope.app"

  zap trash: [
    "~/Library/Application Support/com.circuitscope.app",
    "~/Library/Caches/com.circuitscope.app",
    "~/Library/Logs/com.circuitscope.app",
    "~/Library/Preferences/com.circuitscope.app.plist",
    "~/Library/Saved Application State/com.circuitscope.app.savedState",
    "~/Library/WebKit/com.circuitscope.app",
  ]
end
