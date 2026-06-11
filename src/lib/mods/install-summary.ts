// Build the per-mod dependency lines for the post-install success toast.
//
// Extracted from ModBrowseView's DependencyDialog onConfirm success branch.
// Pure function — no IPC, no side effects.

import type { ModVersion } from '$lib/ipc/bindings';
import type { DepItem, OptionalItem } from '$lib/mods/dep-prompt';

/**
 * Build the deduped list of dependency project names shown in the install
 * success toast, in the same order the DependencyDialog displayed them.
 *
 * Dedup key: `${source}:${project_id}` — first-seen wins.
 *
 * Order:
 * 1. All required deps (in their list order).
 * 2. For each chosen optional (in chosenOptional order):
 *    a. The optional itself.
 *    b. Its transitive requires (in sub-list order).
 *
 * A chosenOptional entry with no matching prompt.optional entry is skipped
 * (defensive; the dialog should only pass versions it knows about).
 */
export function buildInstalledDepLines(
  prompt: { required: DepItem[]; optional: OptionalItem[] },
  chosenOptional: ModVersion[],
): string[] {
  const seen = new Set<string>();
  const depLines: string[] = [];

  const pushDep = (name: string, source: string, projectId: string) => {
    const key = `${source}:${projectId}`;
    if (!seen.has(key)) {
      seen.add(key);
      depLines.push(name);
    }
  };

  for (const d of prompt.required) {
    pushDep(d.projectName, d.version.source, d.version.project_id);
  }

  for (const v of chosenOptional) {
    const o = prompt.optional.find(
      (x) =>
        x.version.source === v.source &&
        x.version.project_id === v.project_id &&
        x.version.version_id === v.version_id,
    );
    if (!o) continue;
    pushDep(o.projectName, o.version.source, o.version.project_id);
    for (const r of o.requires) {
      pushDep(r.projectName, r.version.source, r.version.project_id);
    }
  }

  return depLines;
}
