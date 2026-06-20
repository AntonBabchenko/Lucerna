<script lang="ts">
  import { commands, type MemoryBounds } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import { formatHeapLabel, isAboveRecommended } from '$lib/instances/heap';
  import { serverState } from '$lib/servers/server-state.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';

  let { serverId }: { serverId: string } = $props();

  const running = $derived(serverState.running(serverId));

  let name = $state('');
  let memoryMb = $state(4096);
  let jvmArgs = $state('');
  let initialized = false;

  $effect(() => {
    const s = serverState.list.find((x) => x.id === serverId);
    if (s && !initialized) {
      initialized = true;
      name = s.name;
      memoryMb = s.max_heap_mb;
      jvmArgs = s.extra_jvm_args;
    }
  });

  const FALLBACK_BOUNDS: MemoryBounds = {
    min_mb: 1024,
    max_mb: 8192,
    recommended_max_mb: 8192,
    step_mb: 256,
    ram_known: false,
  };
  let memBounds = $state<MemoryBounds>(FALLBACK_BOUNDS);
  let memBoundsLoaded = false;
  $effect(() => {
    if (memBoundsLoaded) return;
    memBoundsLoaded = true;
    commands
      .instanceMemoryBounds()
      .then((b) => {
        memBounds = b;
        memoryMb = Math.min(Math.max(memoryMb, b.min_mb), b.max_mb);
      })
      .catch(() => {});
  });

  let busy = $state(false);
  let saved = $state(false);
  let error = $state<string | null>(null);

  const canSave = $derived(name.trim().length > 0);

  // Dirty guard (#34): the "Saved" confirmation must not linger once the user
  // starts editing again. We snapshot the fields on a successful save and clear
  // `saved` as soon as the live values diverge from that snapshot.
  const formSig = $derived(JSON.stringify({ name, memoryMb, jvmArgs }));
  let savedSnapshot = $state<string | null>(null);
  $effect(() => {
    if (saved && formSig !== savedSnapshot) saved = false;
  });

  async function save() {
    if (!canSave || busy) return;
    busy = true;
    error = null;
    saved = false;
    try {
      const r1 = await serverState.rename(serverId, name.trim());
      if (!r1.ok) {
        error = formatError(r1.error as Parameters<typeof formatError>[0]);
        return;
      }
      const r2 = await serverState.updateRuntimeConfig(serverId, memoryMb, jvmArgs);
      if (!r2.ok) {
        error = formatError(r2.error as Parameters<typeof formatError>[0]);
        return;
      }
      savedSnapshot = formSig;
      saved = true;
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex flex-col gap-4">
  <div class="flex flex-col gap-1">
    <label for="sg-name" class="text-sm font-medium">{$t('servers.general.name')}</label>
    <input
      id="sg-name"
      type="text"
      maxlength="32"
      class="h-8 rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
      bind:value={name}
    />
  </div>

  <div class="flex flex-col gap-1">
    <!-- svelte-ignore a11y_label_has_associated_control -->
    <label class="text-sm font-medium"
      >{$t('servers.general.memory')} · {formatHeapLabel(memoryMb)}</label
    >
    <input
      type="range"
      min={memBounds.min_mb}
      max={memBounds.max_mb}
      step={memBounds.step_mb}
      value={memoryMb}
      oninput={(e) => (memoryMb = parseInt((e.currentTarget as HTMLInputElement).value, 10))}
      class="w-full"
    />
    {#if isAboveRecommended(memoryMb, memBounds.recommended_max_mb, memBounds.ram_known)}
      <p class="text-xs text-warning-text">
        {$t('instance.manage.memoryWarnHigh', {
          recommended: formatHeapLabel(memBounds.recommended_max_mb),
        })}
      </p>
    {/if}
  </div>

  <div class="flex flex-col gap-1">
    <label for="sg-jvm" class="text-sm font-medium">{$t('servers.general.jvmArgs')}</label>
    <input
      id="sg-jvm"
      type="text"
      class="h-8 rounded border border-border-emphasis bg-surface px-3 font-mono text-xs text-primary"
      bind:value={jvmArgs}
    />
    <p class="text-xs text-muted">{$t('servers.general.jvmArgsHint')}</p>
  </div>

  {#if running}
    <p class="text-xs text-warning-text">{$t('servers.general.restartToApply')}</p>
  {/if}

  <div class="flex items-center gap-3">
    <BusyButton class="btn-primary btn-sm" {busy} disabled={!canSave} onclick={() => void save()}>
      {$t('servers.general.save')}
    </BusyButton>
    {#if saved}
      <span class="text-xs text-success">{$t('servers.general.saved')}</span>
    {/if}
    {#if error}
      <span class="text-xs text-danger">{error}</span>
    {/if}
  </div>
</div>
