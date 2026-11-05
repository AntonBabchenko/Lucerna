<script lang="ts">
  // One `missing_mods` entry in the Finish-install dialog (spec §5.3 item 3): name, the pack's
  // file name and size, kind and optional badges, why it is here, its state, and what opens it.
  import { formatSize } from '$lib/format/size';
  import { t } from '$lib/i18n';
  import type { MissingModStatus } from '$lib/ipc/bindings';
  import { Icon } from '$lib/ui/icons';
  import { openExternalHttps } from '$lib/ui/safe-open';
  import { canCheck, entryLink, LINK_LABEL, reasonText } from './entry-view';

  let {
    status,
    found = null,
    onUseFile,
    onReplace,
  }: {
    status: MissingModStatus;
    /** A file the watch or a drop found that waits for Replace: its occupant's name. */
    found?: string | null;
    onUseFile: () => void;
    onReplace: () => void;
  } = $props();

  const e = $derived(status.entry);
  const link = $derived(entryLink(e));
  const kind = $derived(
    e.meta.install_path?.startsWith('resourcepacks/')
      ? 'resourcePack'
      : e.meta.install_path?.startsWith('shaderpacks/')
        ? 'shader'
        : null,
  );
  const done = $derived(
    status.state === 'installed' ||
      status.state === 'substituted' ||
      status.state === 'chosen_unverified',
  );
  // Nothing published to check a file against: the user may still point at one (spec D8).
  const canChoose = $derived(!canCheck(e) && e.reason !== 'unsupported_type');
</script>

<li class="flex items-start gap-3 px-3 py-2" data-testid="manual-row" data-state={status.state}>
  <Icon
    name={done ? 'success' : 'warning'}
    size={14}
    class="mt-0.5 shrink-0 {done ? 'text-success' : 'text-warning-text'}"
  />
  <div class="min-w-0 flex-1">
    <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
      <span class="text-sm text-primary font-medium">{e.mod_name}</span>
      {#if kind}
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-subtle text-secondary"
          >{$t(
            kind === 'shader' ? 'modpacks.manual.kind.shader' : 'modpacks.manual.kind.resourcePack',
          )}</span
        >
      {/if}
      {#if !e.meta.required}
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-subtle text-secondary"
          >{$t('modpacks.manual.kind.optional')}</span
        >
      {/if}
    </div>
    <div class="text-xs text-muted truncate">
      {e.filename}{#if e.size > 0}&nbsp;· {formatSize($t, e.size)}{/if}
    </div>
    {#if !done}
      <div class="text-xs text-secondary">{reasonText($t, e)}</div>
    {/if}
    <div class="text-xs mt-0.5" data-testid="manual-row-status">
      {#if found}
        <span class="text-warning-text">{$t('modpacks.manual.state.found', { file: found })}</span>
      {:else if status.state === 'installed'}
        <span class="text-success">{$t('modpacks.manual.state.added')}</span>
      {:else if status.state === 'chosen_unverified'}
        <span class="text-secondary">{$t('modpacks.manual.state.chosen')}</span>
      {:else if status.state === 'substituted'}
        <span class="text-secondary">{$t('modpacks.imported.detail.substituted')}</span>
      {:else if status.state === 'different_version'}
        <span class="text-warning-text">{$t('modpacks.manual.state.other')}</span>
      {:else if canChoose}
        <span class="text-warning-text">{$t('modpacks.manual.state.unchecked')}</span>
      {:else}
        <span class="text-warning-text">{$t('modpacks.manual.state.needs')}</span>
      {/if}
    </div>
  </div>
  {#if !done}
    <div class="flex shrink-0 flex-wrap items-center justify-end gap-2">
      {#if found}
        <button type="button" class="btn-warning btn-xs" onclick={onReplace}
          >{$t('modpacks.manual.action.replace')}</button
        >
      {/if}
      {#if canChoose}
        <button type="button" class="btn-secondary btn-xs" onclick={onUseFile}
          >{$t('modpacks.manual.action.useFile')}</button
        >
      {/if}
      {#if link}
        <button
          type="button"
          class="btn-link text-xs inline-flex items-center gap-1"
          onclick={() => void openExternalHttps(link.url)}
        >
          {$t(LINK_LABEL[link.kind])}
          <Icon name="externalLink" size={12} />
        </button>
      {/if}
    </div>
  {/if}
</li>
