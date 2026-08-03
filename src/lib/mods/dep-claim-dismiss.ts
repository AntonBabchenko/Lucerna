// "The author marked this dependency required" is a CLAIM, not a finding: the
// loader enforces only what the jar descriptor says, and a measured mod declares
// a required dependency on Modrinth that its own `neoforge.mods.toml` does not.
// The launcher shows the claim with attribution rather than adopting it, and
// lets the user settle it.
//
// Storage is `diagnosisDismiss` unchanged — its key is opaque and its
// signature semantics are exactly the ones wanted here.

import type { ModSource } from '$lib/ipc/bindings';
import { diagnosisDismiss } from '$lib/ui/diagnosis-dismiss.svelte';

type Ref = { source: ModSource; project_id: string };

// What the user acknowledged: "the author marks this one required". If the
// author later downgrades it, no note renders at all, so the entry simply stops
// matching — the same signature semantics the diagnosis banners use.
const SIGNATURE = 'required';

/**
 * Keyed on the (mod, dependency) PAIR and nothing else — deliberately not
 * per-instance. The claim is a property of the pair: the same wrong platform
 * entry appears in every profile carrying that mod, so acknowledging it once
 * should settle it everywhere. The source is part of both halves so a Modrinth
 * and a CurseForge project that happen to share an id cannot collide.
 */
export function depClaimKey(mod: Ref, dep: Ref): string {
  return `dep:${mod.source}:${mod.project_id}:${dep.source}:${dep.project_id}`;
}

export function isClaimDismissed(mod: Ref, dep: Ref): boolean {
  return diagnosisDismiss.isDismissed(depClaimKey(mod, dep), SIGNATURE);
}

export function dismissClaim(mod: Ref, dep: Ref): void {
  diagnosisDismiss.dismiss(depClaimKey(mod, dep), SIGNATURE);
}

export function restoreClaim(mod: Ref, dep: Ref): void {
  diagnosisDismiss.restore(depClaimKey(mod, dep));
}
