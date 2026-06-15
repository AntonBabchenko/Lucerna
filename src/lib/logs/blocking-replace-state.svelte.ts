// Session-scoped memory for the blocking card's per-mod version UI, so a chosen
// version (and the fetched version list) survives closing/reopening the Logs
// window. Keyed by `${instanceId}::${sha1}`. Lost on launcher restart.

import { SvelteMap } from 'svelte/reactivity';
import type { ModVersion } from '$lib/ipc/bindings';

const loaded = new SvelteMap<string, ModVersion[]>();
const chosen = new SvelteMap<string, string>();

const key = (instanceId: string, sha1: string) => `${instanceId}::${sha1}`;

export function getLoadedVersions(instanceId: string, sha1: string): ModVersion[] | null {
  return loaded.get(key(instanceId, sha1)) ?? null;
}
export function setLoadedVersions(instanceId: string, sha1: string, versions: ModVersion[]): void {
  loaded.set(key(instanceId, sha1), versions);
}
export function getChosenVersion(instanceId: string, sha1: string): string | null {
  return chosen.get(key(instanceId, sha1)) ?? null;
}
export function setChosenVersion(instanceId: string, sha1: string, versionId: string): void {
  chosen.set(key(instanceId, sha1), versionId);
}

/** Test-only: clear all session state so tests don't leak into each other. */
export function __resetBlockingReplaceStateForTest(): void {
  loaded.clear();
  chosen.clear();
}
