<script lang="ts">
  import { onMount } from 'svelte';
  import { commands } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import { getProperty, buildPropertiesText } from '$lib/servers/properties-edit';
  import { SavedForm } from './saved-form.svelte';
  import PropertyField from './PropertyField.svelte';
  import {
    PROPERTIES_CATALOG,
    PROP_GROUP_ORDER,
    keyToI18n,
    type PropGroup,
    type PropSpec,
  } from './properties-catalog';

  let { serverId, running }: { serverId: string; running: boolean } = $props();

  const CATALOG_KEYS = new Set(PROPERTIES_CATALOG.map((s) => s.key));
  const DEFAULTS: Record<string, string> = Object.fromEntries(
    PROPERTIES_CATALOG.map((s) => [s.key, s.default]),
  );

  let raw = $state('');
  let loadError = $state<string | null>(null);
  let saveError = $state<string | null>(null);
  let busy = $state(false);
  let search = $state('');
  let onlyChanged = $state(false);
  let previewOpen = $state(false);
  const saved = new SavedForm();

  // Value map: every catalog key + every unknown file key.
  let values = $state<Record<string, string>>({});
  let otherKeys = $state<string[]>([]);

  function hydrate(text: string) {
    const next: Record<string, string> = {};
    for (const s of PROPERTIES_CATALOG) {
      next[s.key] = getProperty(text, s.key) ?? s.default;
    }
    // Discover unknown keys present in the file.
    const others: string[] = [];
    for (const line of text.split(/\r?\n/)) {
      const trimmed = line.trimStart();
      if (trimmed.startsWith('#') || !trimmed.includes('=')) continue;
      const k = line.slice(0, line.indexOf('='));
      if (!CATALOG_KEYS.has(k) && !others.includes(k)) {
        others.push(k);
        next[k] = getProperty(text, k) ?? '';
      }
    }
    values = next;
    otherKeys = others;
  }

  onMount(async () => {
    const res = await commands.serverReadProperties(serverId);
    if (res.status === 'ok') {
      raw = res.data;
      hydrate(raw);
    } else {
      loadError = formatError(res.error);
    }
  });

  const sig = $derived(JSON.stringify(values));
  $effect(() => saved.sync(sig));

  function setValue(key: string, value: string) {
    values = { ...values, [key]: value };
  }

  function matches(spec: PropSpec): boolean {
    if (onlyChanged && values[spec.key] === spec.default) return false;
    const q = search.trim().toLowerCase();
    if (!q) return true;
    const i18n = keyToI18n(spec.key);
    const hay = [
      spec.key,
      $t(`servers.props.${i18n}.label` as never),
      $t(`servers.props.${i18n}.desc` as never),
    ]
      .join(' ')
      .toLowerCase();
    return hay.includes(q);
  }

  function specsFor(group: PropGroup): PropSpec[] {
    return PROPERTIES_CATALOG.filter((s) => s.group === group && matches(s));
  }

  const visibleGroups = $derived(
    PROP_GROUP_ORDER.map((g) => ({ group: g, specs: specsFor(g) })).filter(
      (x) => x.specs.length > 0,
    ),
  );

  const otherFiltered = $derived(
    otherKeys.filter((k) => {
      if (onlyChanged) return false; // no default to compare against
      const q = search.trim().toLowerCase();
      return !q || k.toLowerCase().includes(q);
    }),
  );

  const anyVisible = $derived(visibleGroups.length > 0 || otherFiltered.length > 0);
  const preview = $derived(buildPropertiesText(raw, values, DEFAULTS));

  async function save() {
    busy = true;
    saveError = null;
    try {
      const text = buildPropertiesText(raw, values, DEFAULTS);
      const res = await commands.serverWriteProperties(serverId, text);
      if (res.status === 'ok') {
        raw = text;
        saved.markSaved(sig);
      } else {
        saveError = formatError(res.error);
      }
    } finally {
      busy = false;
    }
  }
</script>

<details>
  <summary class="inline-flex items-center gap-2 cursor-pointer select-none font-semibold py-1">
    <span class="disclosure-caret"><Icon name="caret" size={14} /></span>
    {$t('servers.propsUi.title')}
  </summary>
  <div class="flex flex-col gap-3 mt-2">
    <div class="flex items-center gap-3 flex-wrap">
      <input
        type="search"
        placeholder={$t('servers.propsUi.searchPlaceholder')}
        class="h-8 flex-1 min-w-[180px] rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        bind:value={search}
      />
      <label class="flex items-center gap-1.5 text-sm text-secondary whitespace-nowrap">
        <input type="checkbox" class="accent-accent cursor-pointer" bind:checked={onlyChanged} />
        {$t('servers.propsUi.onlyChanged')}
      </label>
    </div>

    {#if loadError}
      <p class="text-sm text-danger" role="alert">{loadError}</p>
    {/if}

    {#if !anyVisible}
      <p class="text-sm text-muted">{$t('servers.propsUi.noResults')}</p>
    {/if}

    {#each visibleGroups as { group, specs } (group)}
      <div class="flex flex-col">
        <h4 class="text-xs font-semibold uppercase tracking-wide text-muted mt-2 mb-1">
          {$t(`servers.propGroups.${group}` as never)}
        </h4>
        <div class="divide-y divide-border-subtle">
          {#each specs as spec (spec.key)}
            <PropertyField
              {spec}
              value={values[spec.key] ?? spec.default}
              onChange={(v) => setValue(spec.key, v)}
            />
          {/each}
        </div>
      </div>
    {/each}

    {#if otherFiltered.length > 0}
      <div class="flex flex-col">
        <h4 class="text-xs font-semibold uppercase tracking-wide text-muted mt-2 mb-1">
          {$t('servers.propGroups.other')}
        </h4>
        <p class="text-xs text-muted mb-1">{$t('servers.propsUi.otherNote')}</p>
        <div class="grid grid-cols-[minmax(0,1fr)_minmax(0,320px)] items-center gap-x-4 gap-y-2">
          {#each otherFiltered as k (k)}
            <code class="text-xs text-secondary truncate">{k}</code>
            <input
              type="text"
              aria-label={k}
              class="h-8 w-full rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
              value={values[k] ?? ''}
              oninput={(e) => setValue(k, e.currentTarget.value)}
            />
          {/each}
        </div>
      </div>
    {/if}

    <details class="mt-1" bind:open={previewOpen}>
      <summary class="inline-flex items-center cursor-pointer text-sm text-secondary select-none">
        <span class="disclosure-caret mr-1"><Icon name="caret" size={14} /></span>
        {$t('servers.propsUi.preview')}
      </summary>
      {#if previewOpen}
        <pre
          class="mt-2 max-h-64 overflow-auto rounded border border-border-emphasis bg-base px-2 py-1 font-mono text-xs text-primary whitespace-pre-wrap">{preview}</pre>
      {/if}
    </details>

    {#if running}
      <p class="text-xs text-warning-text">{$t('servers.propsUi.restartToApply')}</p>
    {/if}

    <div class="flex items-center gap-3">
      <BusyButton
        class="btn-primary btn-sm"
        {busy}
        data-testid="properties-save"
        onclick={() => void save()}
      >
        {$t('servers.propsUi.save')}
      </BusyButton>
      {#if saved.saved}
        <span class="text-xs text-success">{$t('servers.propsUi.saved')}</span>
      {/if}
      {#if saveError}
        <span class="text-xs text-danger">{saveError}</span>
      {/if}
    </div>
  </div>
</details>
