<script lang="ts">
  // One API-key field for both Settings forms (CurseForge, AI translation):
  // status line [+ detail, + action] → [disclosure] → guide → field → Save /
  // Clear [+ caption] → result → note. The parent renders its own h3 and intro
  // above, like every other Settings section. Presentational: the parents own
  // the IPC, the strings and the meaning of every state; this component owns
  // the order, the tones, Enter-to-save and one live region per element.
  // DESIGN.md "API-key fields".
  import type { Snippet } from 'svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { Icon } from '$lib/ui/icons';
  import { STATUS_TONE_CLASS, type FieldResult, type FieldStatus } from './api-key-field';

  let {
    statusLabel,
    status,
    statusDetail,
    statusAction,
    collapsed,
    guide,
    inputLabel,
    placeholder,
    value = $bindable(''),
    disabled = false,
    saveLabel,
    onSave,
    saving = false,
    clearLabel,
    onClear,
    clearing = false,
    clearCaption,
    result = null,
    resultTestId,
    note,
    testIdPrefix,
    focusTarget = false,
  }: {
    statusLabel: string;
    /** Fact A — what is stored. `busy` = the read is in flight. */
    status: FieldStatus;
    /** A second, muted line under the status (a keyring reason). */
    statusDetail?: string;
    /** "Check again" and the like, rendered next to the status. */
    statusAction?: { label: string; onClick: () => void };
    /** Wraps guide + field + buttons in a closed <details>; the status stays outside. */
    collapsed?: { summary: string; testId: string };
    /** Steps, "Get a key" links — rendered between the status and the field. */
    guide?: Snippet;
    inputLabel: string;
    placeholder: string;
    value?: string;
    /** Disables the input, Save and Clear alike (consent off, status unknown). */
    disabled?: boolean;
    saveLabel: string;
    onSave: () => void;
    saving?: boolean;
    clearLabel?: string;
    onClear?: () => void;
    clearing?: boolean;
    clearCaption?: string;
    /** Fact B — what happened to what was just typed. */
    result?: FieldResult | null;
    resultTestId?: string;
    /** The keyring sentence. */
    note: string;
    testIdPrefix: 'cf-key' | 'ai-key';
    /** Marks the input as the control a deep link focuses (`data-flash-focus`). */
    focusTarget?: boolean;
  } = $props();

  const busyAny = $derived(saving || clearing);
  const canSave = $derived(!disabled && !busyAny && value.trim() !== '');
  const showClear = $derived(Boolean(clearLabel && onClear));

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && canSave) {
      e.preventDefault();
      onSave();
    }
  }
</script>

{#snippet body()}
  {#if guide}{@render guide()}{/if}
  <label class="block">
    <span class="text-xs text-muted">{inputLabel}</span>
    <input
      type="password"
      autocomplete="off"
      spellcheck="false"
      class="w-full border border-border-emphasis rounded px-3 py-1.5 text-sm font-mono disabled:opacity-50 disabled:cursor-not-allowed"
      {placeholder}
      bind:value
      disabled={disabled || busyAny}
      onkeydown={onKeydown}
      data-testid="{testIdPrefix}-input"
      data-flash-focus={focusTarget ? '' : undefined}
    />
  </label>
  <div class="flex gap-2 mt-3">
    <BusyButton
      type="button"
      class="btn-primary btn-sm"
      busy={saving}
      disabled={!canSave}
      onclick={onSave}
      data-testid="{testIdPrefix}-save"
    >
      {saveLabel}
    </BusyButton>
    {#if showClear}
      <BusyButton
        type="button"
        class="btn-secondary btn-sm"
        busy={clearing}
        disabled={disabled || busyAny}
        onclick={onClear}
        data-testid="{testIdPrefix}-clear"
      >
        {clearLabel}
      </BusyButton>
    {/if}
  </div>
  {#if showClear && clearCaption}
    <p class="text-xs text-muted mt-1">{clearCaption}</p>
  {/if}
{/snippet}

<div class="flex flex-col gap-2">
  <div class="text-sm">
    <span class="text-muted">{statusLabel} </span>
    {#if status.busy}
      <!-- One live region: the Spinner owns role="status" and the label; no
           second sr-only "Loading…", so the state is announced once. -->
      <span
        class="inline-flex align-middle {STATUS_TONE_CLASS[status.tone]}"
        data-testid="{testIdPrefix}-status"
      >
        <Spinner size="sm" label={status.text} labelPlacement="right" />
      </span>
    {:else}
      <span role="status" class={STATUS_TONE_CLASS[status.tone]} data-testid="{testIdPrefix}-status"
        >{status.text}</span
      >
    {/if}
    {#if statusAction}
      <button type="button" class="btn-tertiary btn-sm ml-2" onclick={statusAction.onClick}>
        {statusAction.label}
      </button>
    {/if}
    {#if statusDetail}
      <span class="block text-xs text-muted" data-testid="{testIdPrefix}-status-reason"
        >{statusDetail}</span
      >
    {/if}
  </div>
  {#if collapsed}
    <details data-testid={collapsed.testId}>
      <summary class="cursor-pointer text-sm text-primary inline-flex items-center gap-1">
        <Icon name="caret" class="disclosure-caret" />{collapsed.summary}
      </summary>
      <div class="mt-2">{@render body()}</div>
    </details>
  {:else}
    {@render body()}
  {/if}
  <div data-testid={resultTestId}>
    <StatusMessage
      message={result?.text ?? null}
      tone={result?.tone ?? 'info'}
      live={result && result.tone !== 'info' ? 'assertive' : undefined}
    />
  </div>
  <p class="text-xs text-muted">{note}</p>
</div>
