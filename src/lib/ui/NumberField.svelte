<script lang="ts">
  // A labelled whole-number field with one rule: it never accepts a value it
  // would have to change. It shows the persisted `value`; a change either
  // commits exactly what was typed (once, and only when it differs) or is
  // refused — the saved value is written back and a polite message names what
  // is accepted. It never clamps: a silently altered input is the complaint
  // STOR-05 was about, and a clamp to 1 is the same complaint. Consumers do
  // no validation of their own (spec 10a §3).
  import { t } from '$lib/i18n';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';

  let {
    label,
    value,
    min,
    max,
    hint,
    disabled = false,
    onCommit,
    testId,
  }: {
    label: string;
    /** The persisted value — what the field shows between edits. */
    value: number;
    min: number;
    max?: number;
    /** Rendered under the field and linked through aria-describedby. */
    hint?: string;
    disabled?: boolean;
    /** Called with the typed value when it is accepted and differs from `value`. */
    onCommit: (n: number) => void;
    testId: string;
  } = $props();

  let el = $state<HTMLInputElement | null>(null);
  let message = $state<string | null>(null);
  const hintId = $derived(`${testId}-hint`);

  function accepted(n: number): boolean {
    return Number.isInteger(n) && n >= min && (max === undefined || n <= max);
  }

  function onChange() {
    if (!el) return;
    const raw = el.value.trim();
    const n = raw === '' ? Number.NaN : Number(raw);
    if (!accepted(n)) {
      // Refused: the saved value comes back. Imperative on purpose — the
      // `value` prop has not changed, so a re-render would not touch the input,
      // which still shows what was typed.
      el.value = String(value);
      message =
        max === undefined
          ? $t('common.wholeNumberMin', { min })
          : $t('common.wholeNumberRange', { min, max });
      return;
    }
    message = null;
    if (n !== value) onCommit(n);
  }
</script>

<div class="flex flex-col gap-1">
  <label class="flex flex-col gap-1">
    <span class="text-sm text-primary">{label}</span>
    <input
      bind:this={el}
      type="number"
      step="1"
      inputmode="numeric"
      {min}
      {max}
      {value}
      {disabled}
      aria-describedby={hint ? hintId : undefined}
      class="w-28 border border-border-emphasis rounded px-2 py-1 text-sm disabled:opacity-50 disabled:cursor-not-allowed"
      onchange={onChange}
      data-testid={testId}
    />
  </label>
  <StatusMessage {message} tone="info" />
  {#if hint}
    <p id={hintId} class="text-xs text-muted">{hint}</p>
  {/if}
</div>
