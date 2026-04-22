## Not intended to replicate or infringe copyrights on any other Verilog Compiler.
## Email mepel@umich.edu for questions or concerns, including bugs.


## Everything is a current work in progress, there might very well be significant bugs.

# Circuit Scope

A desktop IDE for **IEEE 1364 Verilog**: edit sources, run simulation to **VCD**, and inspect waveforms. This project targets classic Verilog, not SystemVerilog.

## Stack

- **Frontend:** React, Vite, CodeMirror
- **Desktop:** Tauri 2
- **Compiler / simulator:** Rust crate `verilog-core` (CLI binary `csverilog`)

## Prerequisites

- [Node.js](https://nodejs.org/) (for the UI and npm scripts)
- [Rust](https://rustup.rs/) and the usual [Tauri dependencies](https://v2.tauri.app/start/prerequisites/) for your OS

## Development

```bash
npm install
npm run tauri
```

Web-only UI (no native shell):

```bash
npm run dev
```

## Build

```bash
npm run build
```

Release the desktop app (runs `npm run build` for the UI, then compiles and bundles Tauri):

```bash
npm run build:app
```

Equivalent: `npx tauri build` (uses the CLI from `devDependencies`).

Installers and the `.app` bundle are written under **`src-tauri/target/release/bundle/`** (for example `bundle/dmg/*.dmg` and `bundle/macos/*.app` on Apple Silicon / Intel). If macOS Gatekeeper blocks an unsigned local build, use **right-click → Open** on the app.

If `tauri build` fails with `invalid value '1' for '--ci'`, your environment has `CI=1`. Run `env -u CI npm run build:app` (Unix) or unset `CI` first.

To regenerate icons from a high-resolution master PNG: `npx tauri icon path/to/app-icon.png --output src-tauri/icons`.

Build the standalone Verilog CLI:

```bash
npm run build-csverilog
```
