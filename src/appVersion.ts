import packageJson from "../package.json";

/**
 * Shown in the UI. Source of truth: root `package.json` `version`
 * (keep in sync with `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml`).
 */
export const APP_VERSION: string = packageJson.version;
