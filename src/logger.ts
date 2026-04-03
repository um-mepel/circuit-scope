import { invoke } from "@tauri-apps/api/core";

export async function logAction(event: string, meta?: unknown) {
  try {
    await invoke("log_action", {
      event,
      meta: meta ?? null,
    });
  } catch {
    // Logging must never break the app; swallow errors.
  }
}

