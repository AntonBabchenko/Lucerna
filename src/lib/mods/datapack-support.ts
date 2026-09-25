import { commands } from '$lib/ipc/bindings';

// Session-lived answers for the 1.13 datapack gate (data packs arrived in
// 1.13), shared by AddonsTab's kind switch and WorldsTab's world detail
// dialog. Keyed by instance id and Minecraft version joined with a newline —
// a character neither half can contain. The version is in the key because
// the Manage modal changes an instance's Minecraft IN PLACE (the id never
// changes). Module-level so it survives the remounts both tabs go through:
// without it a pre-1.13 instance would paint the Datapacks surface for one
// frame on every visit before the IPC answer lands.
const cache = new Map<string, boolean>();

const keyOf = (instanceId: string, mcVersion: string | null) => `${instanceId}\n${mcVersion ?? ''}`;

/** The remembered answer, or `true` — uncertainty must not hide the feature. */
export function cachedDatapackSupport(instanceId: string, mcVersion: string | null): boolean {
  return cache.get(keyOf(instanceId, mcVersion)) ?? true;
}

/** Store an answer. Callers do this only after their stale-answer guard passed. */
export function rememberDatapackSupport(
  instanceId: string,
  mcVersion: string | null,
  supported: boolean,
): void {
  cache.set(keyOf(instanceId, mcVersion), supported);
}

/**
 * Ask the backend, which reads `instance.json` itself. An IPC error answers
 * `true`: hiding the feature on a guess would be worse, and every datapack
 * writer refuses a pre-1.13 instance on its own fresh read
 * (`commands::datapacks::require_datapack_support`), so the permissive answer
 * can never write.
 */
export async function resolveDatapackSupport(instanceId: string): Promise<boolean> {
  const r = await commands.instanceSupportsDatapacks(instanceId);
  return r.status === 'ok' ? r.data : true;
}
