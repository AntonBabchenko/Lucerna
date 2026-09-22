<script lang="ts">
  // Push-1 stub: the final prop contract so tests/number-field.test.ts compiles.
  // The behaviour (refuse-or-commit) lands in push 2.
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
    value: number;
    min: number;
    max?: number;
    hint?: string;
    disabled?: boolean;
    onCommit: (n: number) => void;
    testId: string;
  } = $props();
</script>

<label class="flex flex-col gap-1">
  <span class="text-sm text-primary">{label}</span>
  <input type="number" {min} {max} {value} {disabled} data-testid={testId} />
</label>
{#if hint}
  <p class="text-xs text-muted">{hint}</p>
{/if}
