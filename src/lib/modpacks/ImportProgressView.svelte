<script lang="ts">
  import type { ModpackProgress, ProgressTick } from '$lib/ipc/bindings';

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
    switch (p.phase) {
      case 'inspecting':
        return 'Inspecting pack…';
      case 'creating_instance':
        return `Creating instance ${p.name}…`;
      case 'installing_mod':
        return `Installing mod ${p.current}/${p.total}: ${p.mod_name}`;
      case 'extracting_overrides':
        return `Extracting overrides ${p.current}/${p.total}…`;
      case 'done':
        return 'Done.';
    }
  }

  function percent(current: number, total: number): number {
    if (total <= 0) return 0;
    return Math.min(100, Math.round((current / total) * 100));
  }
</script>

{#if phase}
  <div class="p-4" data-testid="import-progress-view">
    <div class="text-sm">{label(phase)}</div>
    {#if modBytes && modBytes.total && modBytes.total > 0 && modBytes.current != null}
      <div class="h-2 bg-neutral-200 rounded mt-2 overflow-hidden">
        <div
          class="h-full bg-blue-500"
          style="width: {percent(modBytes.current, modBytes.total)}%"
        ></div>
      </div>
    {/if}
  </div>
{/if}
