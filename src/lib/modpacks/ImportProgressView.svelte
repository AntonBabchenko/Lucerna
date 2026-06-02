<script lang="ts">
  import { get } from 'svelte/store';
  import type { ModpackProgress, ProgressTick } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';

  // Renders the current modpack import phase + a bytes-progress bar for the
  // per-mod download.
  //
  // Channel-vs-event tradeoff: Task 9's IPC threads the two progress
  // streams as `Channel<ModpackProgress>` and `Channel<ProgressTick>`
  // parameters on `commands.modpackImport`, not as globally emitted
  // events. Channels are bound to a single command invocation, so the
  // parent (`ModpacksTab` in Task 12) is the natural owner — it creates
  // the channels, attaches `.onmessage` handlers that update local
  // `$state`, and passes the latest values down to this view.
  //
  // The "done" transition is the parent's responsibility: when it sees
  // `{ phase: 'done', instance_id }` on the modpack channel it calls the
  // view's `onDone` callback so this component stays render-only.

  let {
    phase,
    modBytes,
  }: {
    phase: ModpackProgress | null;
    modBytes: ProgressTick | null;
  } = $props();

  function label(p: ModpackProgress): string {
    const tr = get(t);
    switch (p.phase) {
      case 'inspecting':
        return tr('modpacks.import.progress.phaseInspecting');
      case 'creating_instance':
        return tr('modpacks.import.progress.phaseCreatingInstance', { name: p.name });
      case 'installing_file':
        return tr('modpacks.import.progress.phaseInstallingFile', {
          current: p.current,
          total: p.total,
          fileName: p.file_name,
        });
      case 'extracting_overrides':
        return tr('modpacks.import.progress.phaseExtractingOverrides', {
          current: p.current,
          total: p.total,
        });
      case 'enriching':
        return tr('modpacks.import.progress.phaseEnriching');
      case 'done':
        return tr('modpacks.import.progress.phaseDone');
    }
  }

  function percent(current: number, total: number): number {
    if (total <= 0) return 0;
    return Math.min(100, Math.round((current / total) * 100));
  }
</script>

{#if phase}
  <div
    class="fixed top-4 right-4 z-40 w-72 bg-surface rounded-lg shadow-xl border p-4"
    role="status"
    aria-label={$t('modpacks.import.progress.ariaLabel')}
    data-testid="import-progress-view"
  >
    <h3 class="font-semibold text-sm text-primary mb-1">
      {$t('modpacks.import.progress.heading')}
    </h3>
    <div class="text-sm text-secondary truncate">{label(phase)}</div>
    {#if modBytes && modBytes.total && modBytes.total > 0 && modBytes.current != null}
      <div class="h-2 bg-subtle rounded mt-2 overflow-hidden">
        <div
          class="h-full bg-accent"
          style="width: {percent(modBytes.current, modBytes.total)}%"
        ></div>
      </div>
    {/if}
  </div>
{/if}
