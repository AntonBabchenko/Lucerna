<script lang="ts">
  // RED STUB (push 1): the field's shape only — status text, input, Save. The
  // real component (order, tones, one live region, disclosure, guide, result,
  // note, Clear) lands in push 2; `tests/api-key-field.test.ts` pins it.
  import type { Snippet } from 'svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import type { FieldResult, FieldStatus } from './api-key-field';

  let {
    statusLabel,
    status,
    inputLabel,
    placeholder,
    value = $bindable(''),
    disabled = false,
    saveLabel,
    onSave,
    saving = false,
    testIdPrefix,
  }: {
    heading?: string;
    statusLabel: string;
    status: FieldStatus;
    statusDetail?: string;
    statusAction?: { label: string; onClick: () => void };
    collapsed?: { summary: string; testId: string };
    guide?: Snippet;
    inputLabel: string;
    placeholder: string;
    value?: string;
    disabled?: boolean;
    saveLabel: string;
    onSave: () => void;
    saving?: boolean;
    clearLabel?: string;
    onClear?: () => void;
    clearing?: boolean;
    clearCaption?: string;
    result?: FieldResult | null;
    resultTestId?: string;
    note: string;
    testIdPrefix: 'cf-key' | 'ai-key';
  } = $props();
</script>

<div>
  <span class="text-muted">{statusLabel} </span>
  <span data-testid="{testIdPrefix}-status">{status.text}</span>
  <label class="block">
    <span class="text-xs text-muted">{inputLabel}</span>
    <input type="password" {placeholder} bind:value {disabled} data-testid="{testIdPrefix}-input" />
  </label>
  <BusyButton
    type="button"
    class="btn-primary btn-sm"
    busy={saving}
    disabled={disabled || value.trim() === ''}
    onclick={onSave}
    data-testid="{testIdPrefix}-save"
  >
    {saveLabel}
  </BusyButton>
</div>
