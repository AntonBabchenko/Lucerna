import { get } from 'svelte/store';
import { reasonLines } from '$lib/format/reason-lines';
import { type Translate, t } from '$lib/i18n';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { DropSkip } from '$lib/layout/drop-router';
import { pushWarning } from '$lib/toasts/toasts.svelte';

// The words for a file a drop does not add (DESIGN.md §14) — shared by the drag preview, the drop's
// toast and a host that refuses a routed drop itself. A `Record`, so a new reason without words is
// a compile error.
const SKIP_KEY: Record<DropSkip, TranslationKey> = {
  nowhere: 'common.dropSkip.nowhere',
  modal: 'common.dropSkip.modal',
  no_instance: 'common.dropSkip.noInstance',
  // A box that takes nothing right now: the words its own disabled strip shows.
  no_mod_loader: 'mods.browse.dropzoneDisabled',
  server_running: 'servers.mods.stopToManage',
  data_root: 'page.dataRootFallback.createDisabledReason',
  only_mods: 'common.dropSkip.onlyMods',
  only_plugins: 'common.dropSkip.onlyPlugins',
  only_resource_packs: 'common.dropSkip.onlyResourcePacks',
  only_shaders: 'common.dropSkip.onlyShaders',
  only_datapacks: 'common.dropSkip.onlyDatapacks',
  only_modpacks: 'common.dropSkip.onlyModpacks',
  one_modpack: 'common.dropSkip.oneModpack',
  one_server: 'common.dropSkip.oneServer',
};

export function skipReason(tr: Translate, why: DropSkip): string {
  return tr(SKIP_KEY[why]);
}

function fileName(path: string): string {
  return (
    path
      .split(/[\\/]/)
      .filter((part) => part !== '')
      .pop() ?? path
  );
}

/**
 * One warning toast for the files a drop did not add: how many, then each reason with the files it
 * left behind on a line above it — the grouping the update reports use (`reasonLines`). A drop is
 * never discarded in silence.
 */
export function reportNotAdded(skipped: readonly { path: string; reason: string }[]): void {
  if (skipped.length === 0) return;
  pushWarning(
    get(t)('common.dropNotAdded', { count: skipped.length }),
    reasonLines(skipped.map((s) => ({ name: fileName(s.path), reason: s.reason }))),
  );
}
