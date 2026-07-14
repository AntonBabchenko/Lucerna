<script lang="ts">
  import { commands, type ChangelogResult, type ModSource } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import Modal from '$lib/ui/Modal.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import RenderedBody from '$lib/ui/RenderedBody.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';

  // Lazy changelog viewer for an update. The owning view mounts this when the
  // user clicks "changelog" and unmounts on close; it fetches once on mount.
  // Only sources that implement `changelog_range` (Modrinth, CurseForge) should
  // open this — callers gate the affordance via `changelogSupported`.
  let {
    source,
    projectId,
    title,
    targetVersionId,
    baseVersionId = null,
    onClose,
  }: {
    source: ModSource;
    projectId: string;
    title: string;
    targetVersionId: string;
    baseVersionId?: string | null;
    onClose: () => void;
  } = $props();

  let result = $state<ChangelogResult | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);

  async function load() {
    loading = true;
    error = null;
    const r = await commands.modsChangelog(source, projectId, targetVersionId, baseVersionId);
    if (r.status === 'ok') {
      result = r.data;
    } else {
      error = formatError(r.error);
    }
    loading = false;
  }

  $effect(() => {
    void targetVersionId;
    void load();
  });

  const allEmpty = $derived(
    result !== null && result.sections.every((s) => s.body_html.trim() === ''),
  );
</script>

<Modal
  ariaLabelledby="changelog-title"
  {onClose}
  panelClass="w-full max-w-2xl lg:max-w-3xl max-h-[90vh] flex flex-col"
>
  <div class="p-4 pb-0 shrink-0 flex items-start justify-between">
    <h2 id="changelog-title" class="text-base font-semibold text-primary flex-1">{title}</h2>
    <CloseButton onClick={onClose} ariaLabel={$t('common.close')} />
  </div>

  <div class="flex-1 overflow-y-auto min-h-0 p-4 pt-3">
    {#if loading}
      <div class="flex justify-center py-8 text-secondary">
        <Spinner labelPlacement="below" label={$t('mods.changelog.loading')} />
      </div>
    {:else if error}
      <div class="bg-danger-bg border border-danger text-danger text-sm rounded p-2 mb-3">
        {error}
      </div>
      <button type="button" class="btn-secondary btn-sm" onclick={() => void load()}>
        {$t('mods.changelog.retry')}
      </button>
    {:else if result && result.sections.length === 0}
      <p class="text-sm text-placeholder">{$t('mods.changelog.empty')}</p>
    {:else if result}
      {#if result.truncated}
        <p class="text-xs text-muted mb-3">
          {$t('mods.changelog.truncated', {
            shown: result.sections.length,
            total: result.truncated,
          })}
        </p>
      {/if}
      {#if allEmpty}
        <p class="text-sm text-placeholder">{$t('mods.changelog.empty')}</p>
      {:else}
        <div class="space-y-4">
          {#each result.sections as section (section.version_id)}
            <div class="border-t border-border-subtle pt-3 first:border-t-0 first:pt-0">
              <div class="flex items-baseline gap-2 mb-1">
                <span class="font-medium text-sm text-primary">{section.version_number}</span>
                {#if section.published_at}
                  <span class="text-xs text-muted">{section.published_at.slice(0, 10)}</span>
                {/if}
              </div>
              {#if section.body_html.trim() === ''}
                <p class="text-xs text-placeholder italic">{$t('mods.changelog.noNotes')}</p>
              {:else}
                <RenderedBody html={section.body_html} />
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</Modal>
