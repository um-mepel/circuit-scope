# Homebrew tap for Circuit Scope

This folder contains the [cask](Casks/circuit-scope.rb) for the Circuit Scope desktop app and the [formula](Formula/csverilog.rb) for the `csverilog` CLI. They are consumed from a separate **Homebrew tap** repository that hosts these two files at their root.

## One-time tap setup

1. Create a public GitHub repository named exactly `homebrew-circuit-scope` (the `homebrew-` prefix is required by Homebrew).
2. Seed the tap from this directory:

   ```bash
   git clone https://github.com/um-mepel/homebrew-circuit-scope.git tap
   cp -R homebrew/Casks homebrew/Formula tap/
   cd tap
   git add Casks Formula
   git commit -m "Initial cask + formula for Circuit Scope"
   git push origin main
   ```

3. End users install with:

   ```bash
   brew tap um-mepel/circuit-scope
   brew install --cask circuit-scope
   brew install csverilog
   ```

## Per-release bump workflow

Each time you publish a new tagged release (e.g. `v0.2.3`):

1. **Cut the GitHub Release** by pushing the `vX.Y.Z` tag. The workflow at [../.github/workflows/release.yml](../.github/workflows/release.yml) builds and attaches:
   - `Circuit Scope_<version>_aarch64.dmg` (Apple Silicon)
   - `Circuit Scope_<version>_x64.dmg` (Intel)
   - `csverilog-<version>-aarch64-apple-darwin.tar.gz` + `.sha256`
   - `csverilog-<version>-x86_64-apple-darwin.tar.gz` + `.sha256`

2. **Compute checksums** locally (or copy them from the `.sha256` sidecar files on the Release page):

   ```bash
   version=0.2.3
   for arch in aarch64 x64; do
     curl -L -o "cs-${arch}.dmg" \
       "https://github.com/um-mepel/circuit-scope-verilog/releases/download/v${version}/Circuit%20Scope_${version}_${arch}.dmg"
     shasum -a 256 "cs-${arch}.dmg"
   done

   curl -L -o "source.tar.gz" \
     "https://github.com/um-mepel/circuit-scope-verilog/archive/refs/tags/v${version}.tar.gz"
   shasum -a 256 source.tar.gz
   ```

3. **Update [Casks/circuit-scope.rb](Casks/circuit-scope.rb)**: bump `version`, replace both `sha256` values (ARM + Intel).

4. **Update [Formula/csverilog.rb](Formula/csverilog.rb)**: bump the `url` tag and replace `sha256` with the tarball checksum from step 2.

5. **Commit + push** to the tap repo. Users running `brew update && brew upgrade` will pick up the new versions automatically.

## Audit and local test

Before publishing the tap, verify everything passes Homebrew's linters and a local install:

```bash
brew tap --force-auto-update um-mepel/circuit-scope
brew audit --cask --new-cask um-mepel/circuit-scope/circuit-scope
brew audit --formula --new-formula um-mepel/circuit-scope/csverilog
brew install --build-from-source um-mepel/circuit-scope/csverilog
brew test um-mepel/circuit-scope/csverilog
brew install --cask um-mepel/circuit-scope/circuit-scope
```

## Optional: upstream to default Homebrew taps

Once the tap is stable and has real usage, you can propose inclusion in the core taps:

- **Cask** → open a PR to [Homebrew/homebrew-cask](https://github.com/Homebrew/homebrew-cask). GitHub-hosted casks must meet the [notability criteria](https://docs.brew.sh/Acceptable-Casks#notable-and-active).
- **Formula** → open a PR to [Homebrew/homebrew-core](https://github.com/Homebrew/homebrew-core). The formula must satisfy the [acceptability checklist](https://docs.brew.sh/Acceptable-Formulae) (stable versioned source, OSI license, passing `brew audit --strict --online`, and a meaningful `test` block).

Keep maintaining the custom tap as a fallback until both PRs land.

## Notes on signing and Gatekeeper

The cask installs an unsigned `.app` by default. First-launch on modern macOS may require the user to right-click → Open once (see [README.md](../README.md)). To ship a signed, notarized bundle later, add an [Apple Developer ID certificate and notarization step](https://v2.tauri.app/distribute/sign/macos/) to [release.yml](../.github/workflows/release.yml) via the `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, and `APPLE_TEAM_ID` secrets (these are read automatically by `tauri-apps/tauri-action`).
