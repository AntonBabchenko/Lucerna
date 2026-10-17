<script lang="ts">
  // The Launch section of the Settings tab: memory, the Advanced disclosure
  // (initial heap and the instance's own Java) and extra JVM arguments. Each
  // field saves on its own when it is left or released; there is no Save button.
  import { onDestroy } from 'svelte';
  import { get } from 'svelte/store';
  import { commands, type Error as IpcError, type InstanceWithStatus } from '$lib/ipc/bindings';
  import { formatHeapLabel } from '$lib/instances/heap';
  import { instanceIpcErrorMessage } from '$lib/instances/instance-ipc-error';
  import JavaPathField from '$lib/instances/JavaPathField.svelte';
  import { shouldFocusField, type ManageFocusField } from '$lib/instances/manage-focus';
  import MemorySlider from '$lib/instances/MemorySlider.svelte';
  import { t } from '$lib/i18n';
  import { fieldFlash } from '$lib/ui/field-flash';
  import { Icon } from '$lib/ui/icons';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { reportLateFailure } from './late-failure';

  let {
    instance,
    running = false,
    focusField = null,
    onFocusDelivered = () => {},
    onChanged,
  }: {
    instance: InstanceWithStatus;
    /** This instance's game is up: what changes here applies at its next start. */
    running?: boolean;
    focusField?: ManageFocusField | null;
    onFocusDelivered?: () => void;
    onChanged: () => void;
  } = $props();

  // See InstanceGameSection: completions after unmount or an instance switch
  // refresh the data but write no UI. Plain `let` (destroy-phase reads).
  let unmounted = false;
  onDestroy(() => {
    unmounted = true;
  });
  function isStale(id: string): boolean {
    return unmounted || instance.id !== id;
  }

  let error = $state<string | null>(null);
  let errorEl = $state<HTMLElement | null>(null);
  let lastScrolledError: string | null = null;
  $effect(() => {
    if (error && !lastScrolledError && errorEl) errorEl.scrollIntoView?.({ block: 'nearest' });
    lastScrolledError = error;
  });

  // Transient per-field "Saved" confirmation: each field saves independently,
  // so the user otherwise gets no signal a blur/drag actually persisted.
  const SAVED_HINT_MS = 1500;
  type SavedField = 'memory' | 'jvm';
  let savedField = $state<SavedField | null>(null);
  let savedTimer: ReturnType<typeof setTimeout> | null = null;
  function markSaved(f: SavedField) {
    savedField = f;
    if (savedTimer) clearTimeout(savedTimer);
    savedTimer = setTimeout(() => {
      if (savedField === f) savedField = null;
    }, SAVED_HINT_MS);
  }
  function clearSaved(f: SavedField) {
    if (savedField === f) savedField = null;
  }
  onDestroy(() => {
    if (savedTimer) clearTimeout(savedTimer);
  });

  // Local heap draft so dragging updates the label live WITHOUT a disk write
  // per tick: onInput updates the draft, and the value persists once on release.
  // Seeded per instance id, so a background refresh (same id, new object) does
  // not reset an active drag; another instance reseeds.
  let heapDraft = $state(0);
  // Optional initial heap (-Xms). null = unset (JVM default). Same seeding.
  let minHeapDraft = $state<number | null>(null);
  let seededId: string | null = null;
  $effect(() => {
    if (instance.id !== seededId) {
      seededId = instance.id;
      heapDraft = instance.max_heap_mb;
      minHeapDraft = instance.min_heap_mb;
      savedField = null;
      error = null;
    }
  });

  // The Java field probes and looks for Javas when it mounts, so it mounts only
  // once Advanced is open, not on every visit to the tab (#508).
  let advancedOpen = $state(false);

  async function save(
    field: SavedField | null,
    label: string,
    write: (id: string) => Promise<{ status: 'ok' } | { status: 'error'; error: IpcError }>,
  ) {
    const { id, name } = instance;
    if (!isStale(id)) error = null;
    const result = await write(id);
    if (result.status === 'ok') {
      onChanged();
      if (field && !isStale(id)) markSaved(field);
      return;
    }
    const message = instanceIpcErrorMessage(result.error);
    if (isStale(id)) reportLateFailure(name, label, message);
    else error = message;
  }

  function setMemory(mb: number) {
    return save('memory', get(t)('page.overview.labelMemory'), (id) =>
      commands.setInstanceMemory(id, mb),
    );
  }

  function setJvmArgs(args: string) {
    return save('jvm', get(t)('instance.manage.jvmArgsLabel'), (id) =>
      commands.setInstanceJvmArgs(id, args),
    );
  }

  async function commitMinHeap() {
    // Empty/0 clears it (None). Cap at the current max heap — an Xms larger than
    // Xmx makes the JVM refuse to start.
    const raw = minHeapDraft && minHeapDraft > 0 ? minHeapDraft : null;
    const value = raw === null ? null : Math.min(raw, heapDraft);
    minHeapDraft = value; // reflect the clamp/clear back into the field
    if (value === instance.min_heap_mb) return; // unchanged — no write
    await save(null, get(t)('instance.manage.minHeapLabel'), (id) =>
      commands.setInstanceMinHeap(id, value),
    );
  }
</script>

{#snippet savedBadge(field: SavedField)}
  {#if savedField === field}
    <span class="inline-flex items-center gap-1 font-normal text-success">
      <Icon name="success" size={12} />
      {$t('instance.manage.saved')}
    </span>
  {/if}
{/snippet}

<section class="flex flex-col">
  <h3 class="mb-2 text-sm font-medium text-primary">{$t('instance.settings.launchSection')}</h3>
  {#if running}
    <!-- The server tab's «Restart the server to apply…» sibling: memory, Java and JVM arguments
         stay editable while the game runs, and the game keeps what it started with. -->
    <p class="mb-2 text-xs text-muted" data-testid="launch-applies-next-start">
      {$t('instance.settings.launchNextStart')}
    </p>
  {/if}

  <div
    data-focus-field="memory"
    use:fieldFlash={{
      active: focusField === 'memory',
      focus: shouldFocusField('memory'),
      onDelivered: onFocusDelivered,
    }}
  >
    <label
      for="detail-memory"
      class="mb-1 flex items-center justify-between text-xs text-secondary"
    >
      <span>{$t('instance.manage.memoryLabel', { value: formatHeapLabel($t, heapDraft) })}</span>
      {@render savedBadge('memory')}
    </label>
    <MemorySlider
      id="detail-memory"
      class="mb-1"
      warnClass="mb-3"
      reserveWarnSpace
      valueMb={heapDraft}
      onInput={(mb) => {
        heapDraft = mb;
        clearSaved('memory');
      }}
      onCommit={(mb) => setMemory(mb)}
    />
  </div>

  <details class="mb-3" bind:open={advancedOpen}>
    <summary class="cursor-pointer select-none text-xs text-secondary">
      {$t('instance.manage.advancedSummary')}
    </summary>
    <div class="mt-2">
      <div class="mb-1 flex items-center justify-between">
        <label for="detail-min-heap" class="text-xs text-secondary">
          {$t('instance.manage.minHeapLabel')}
        </label>
        <button
          type="button"
          class="btn-link text-xs"
          onclick={() => {
            minHeapDraft = heapDraft;
            void commitMinHeap();
          }}
        >
          {$t('instance.manage.minHeapEqualsMax')}
        </button>
      </div>
      <input
        id="detail-min-heap"
        type="number"
        class="border rounded px-2 py-1 w-full text-sm"
        min="0"
        max={heapDraft}
        placeholder={$t('instance.manage.minHeapPlaceholder')}
        bind:value={minHeapDraft}
        onchange={commitMinHeap}
      />
      <p class="mt-1 text-xs text-placeholder">{$t('instance.manage.minHeapHint')}</p>
      {#if advancedOpen}
        <JavaPathField
          instanceId={instance.id}
          value={instance.java}
          mcVersion={instance.mc_version}
          loader={instance.loader}
          loaderVersion={instance.loader_version}
          ready={instance.ready}
          onChanged={() => onChanged()}
        />
      {/if}
    </div>
  </details>

  <label
    for="detail-jvm-args"
    class="mb-1 flex items-center justify-between text-xs text-secondary"
  >
    <span>{$t('instance.manage.jvmArgsLabel')}</span>
    {@render savedBadge('jvm')}
  </label>
  <input
    id="detail-jvm-args"
    class="border rounded px-2 py-1 w-full mb-3 font-mono text-xs"
    placeholder={$t('instance.manage.jvmArgsPlaceholder')}
    value={instance.extra_jvm_args}
    oninput={() => clearSaved('jvm')}
    onchange={(e) => setJvmArgs((e.currentTarget as HTMLInputElement).value)}
  />

  <StatusMessage bind:element={errorEl} tone="danger" message={error} class="mb-2" />
</section>
