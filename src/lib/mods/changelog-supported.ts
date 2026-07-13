import type { ModSource } from '$lib/ipc/bindings';

// Mirror of the Rust `changelog_supported`: only these sources implement
// `changelog_range`, so only they get a "changelog" affordance. Keep in sync
// with src-tauri/src/mods/platform.rs.
export function changelogSupported(source: ModSource): boolean {
  return source === 'modrinth' || source === 'curseforge';
}
