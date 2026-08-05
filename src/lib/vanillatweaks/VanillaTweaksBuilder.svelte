<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity';
  import { commands, type VtCategory } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/ipc/format-error';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import { conflictsFor, packId, toSelection } from '$lib/vanillatweaks/vt-selection';

  // The Vanilla Tweaks builder. Not a catalogue browser: VT has no project
  // ids, no search and no pagination — you tick packs and the site builds a
  // zip. So there is no ModResultsGrid, no SourcePicker and no Pagination
  // here, and that absence is the design, not an omission.
  //
  // The tick state for already-installed packs comes from `installed`, which
  // the host derives from the registry rows. The builder stores no selection
  // of its own — the registry is the record of what is installed.

  let {
    mcVersion,
    installed,
    busy = false,
    onBuild,
  }: {
    mcVersion: string;
    /** packId → installed version, from the host's registry rows. */
    installed: Map<string, string>;
    busy?: boolean;
    onBuild: (selection: [string, string[]][]) => Promise<void>;
  } = $props();

  let categories = $state<VtCategory[]>([]);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  const ticked = new SvelteSet<string>();

  $effect(() => {
    const mc = mcVersion;
    let cancelled = false;
    loading = true;
    loadError = null;
    void (async () => {
      const res = await commands.vtCatalogue(mc);
      if (cancelled) return;
      if (res.status === 'ok') {
        categories = res.data.categories;
        loadError = null;
      } else {
        categories = [];
        loadError = formatError(res.error);
      }
      loading = false;
    })();
    return () => {
      cancelled = true;
    };
  });

  const selection = $derived(toSelection(categories, ticked));
  const selectedCount = $derived(selection.reduce((n, [, names]) => n + names.length, 0));

  function toggle(id: string) {
    if (ticked.has(id)) ticked.delete(id);
    else ticked.add(id);
  }
</script>

<div class="flex flex-col gap-4" data-testid="vt-builder">
  <header class="flex flex-col gap-1">
    <h2 class="text-lg font-semibold">{$t('addons.datapacks.vt.title')}</h2>
    <p class="text-sm text-muted">{$t('addons.datapacks.vt.subtitle')}</p>
  </header>

  {#if loadError}
    <p class="text-sm text-danger" role="alert">{loadError}</p>
  {:else if loading}
    <LoadingPanel label={$t('addons.datapacks.vt.title')} delayMs={0} />
  {:else if categories.length === 0}
    <p class="py-6 text-center text-sm text-muted">{$t('addons.datapacks.vt.empty')}</p>
  {:else}
    <div class="flex max-h-[60vh] flex-col gap-5 overflow-y-auto pr-1">
      {#each categories as cat (cat.category)}
        <section class="flex flex-col gap-2">
          <h3 class="text-sm font-semibold uppercase tracking-wide text-muted">
            {cat.category}
          </h3>
          <ul class="flex flex-col gap-1">
            {#each cat.packs as pack (pack.name)}
              {@const id = packId(cat.category, pack)}
              {@const conflicts = conflictsFor(pack, categories, ticked)}
              <li class="rounded-md px-2 py-1.5 hover:bg-surface-2">
                <label class="flex cursor-pointer items-start gap-2.5">
                  <input
                    type="checkbox"
                    class="mt-1 accent-accent"
                    checked={ticked.has(id)}
                    onchange={() => toggle(id)}
                    data-testid="vt-pack-{id}"
                  />
                  <span class="flex min-w-0 flex-col gap-0.5">
                    <span class="flex flex-wrap items-baseline gap-2">
                      <span class="text-sm font-medium">{pack.display}</span>
                      <span class="text-xs text-muted">v{pack.version}</span>
                      {#if installed.has(id)}
                        <span class="text-xs text-accent">
                          {$t('addons.datapacks.vt.installed')}
                        </span>
                      {/if}
                    </span>
                    {#if pack.description}
                      <span class="text-xs text-muted">{pack.description}</span>
                    {/if}
                    {#if conflicts.length > 0}
                      <!-- A warning, never a block: VT's own site allows the
                           combination, and we cannot verify it against the
                           user's actual world. -->
                      <span class="text-xs text-warning">
                        {$t('addons.datapacks.vt.conflicts', { names: conflicts.join(', ') })}
                      </span>
                    {/if}
                  </span>
                </label>
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>

    <footer class="flex items-center justify-between gap-3 border-t border-subtle pt-3">
      <span class="text-sm text-muted">
        {$t('addons.datapacks.vt.selected', { count: selectedCount })}
      </span>
      <button
        type="button"
        class="btn-primary"
        disabled={busy || selectedCount === 0}
        onclick={() => void onBuild(selection)}
        data-testid="vt-build"
      >
        {$t('addons.datapacks.vt.build')}
      </button>
    </footer>
  {/if}
</div>
