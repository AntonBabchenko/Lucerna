<script lang="ts">
  // The added / updated / removed file list for a modpack version change.
  // Shared by the update-confirm dialog and the version-switch dialog so the two
  // surfaces cannot drift. `data-testid="update-diff-list"` is carried over from
  // the original inline markup — existing tests key on it.
  import type { ModpackUpdateDiff } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';

  let { diff }: { diff: ModpackUpdateDiff } = $props();

  const isEmpty = $derived(diff.added.length + diff.updated.length + diff.removed.length === 0);
</script>

<div class="flex-1 overflow-y-auto border rounded divide-y text-sm" data-testid="update-diff-list">
  {#each diff.added as f (f.install_path)}
    <div class="px-2 py-1 text-success flex items-center gap-1.5">
      <Icon name="plus" />
      {f.name}
    </div>
  {/each}
  {#each diff.updated as e (e.new.install_path)}
    <div class="px-2 py-1 text-accent flex items-center gap-1.5">
      <Icon name="update" />
      {e.new.name}
    </div>
  {/each}
  {#each diff.removed as f (f.install_path)}
    <div class="px-2 py-1 text-danger line-through flex items-center gap-1.5">
      <Icon name="minus" />
      {f.name}
    </div>
  {/each}
  {#if isEmpty}
    <div class="px-2 py-3 text-muted text-center">{$t('modpacks.update.noChanges')}</div>
  {/if}
</div>
