import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const manifest = path.join(root, "src-tauri/verilog-core/Cargo.toml");
const coreDir = path.join(root, "src-tauri/verilog-core");
const args = process.argv.slice(2);

const isWin = process.platform === "win32";
const ext = isWin ? ".exe" : "";
const debugExe = path.join(coreDir, "target", "debug", `csverilog${ext}`);
const releaseExe = path.join(coreDir, "target", "release", `csverilog${ext}`);

function run(exe) {
  const code = spawnSync(exe, args, {
    stdio: "inherit",
    shell: false,
  });
  if (code.error) {
    console.error(String(code.error.message || code.error));
    process.exit(1);
  }
  process.exit(code.status ?? 1);
}

// Prefer a built binary: `cargo run` from the integrated terminal floods the WebView
// with rustc output and can crash the UI; a direct exec only prints csverilog itself.
if (existsSync(debugExe)) {
  run(debugExe);
} else if (existsSync(releaseExe)) {
  run(releaseExe);
} else {
  const code = spawnSync(
    "cargo",
    ["run", "-q", "--manifest-path", manifest, "--bin", "csverilog", "--", ...args],
    {
      stdio: "inherit",
      shell: false,
      env: {
        ...process.env,
        CARGO_TERM_COLOR: "never",
      },
    }
  );
  if (code.error) {
    console.error(String(code.error.message || code.error));
    process.exit(1);
  }
  process.exit(code.status ?? 1);
}
