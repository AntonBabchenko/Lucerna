<script lang="ts">
  import { t } from '$lib/i18n';
  import { dismiss, toastList } from '$lib/toasts/toasts.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';

  // Renders the active toast stack in the top-right corner. Mounted once
  // at the app root (`src/routes/+page.svelte`). z-[200] keeps toasts
  // above EVERYTHING — modals (z-40-50), contextual tour overlays
  // (z-100/101), drag-drop dropzone highlights. Toasts are the only
  // chrome that announces transient state; they must never be obscured.
  const toasts = $derived(toastList());
  const dismissLabel = $derived($t('common.dismissNotification'));
</script>

<!-- The aria-live region is ALWAYS in the DOM (even with zero toasts) so a
     screen reader has it registered before the first toast fires — a region
     inserted at the same moment its first child appears can be missed. Only
     the individual toast children are conditional. `aria-atomic="false"` so
     each newly-added toast is announced on its own rather than re-reading the
     whole stack. -->
<div
  class="fixed top-4 right-4 z-[200] flex flex-col gap-2"
  data-testid="toast-host"
  aria-live="polite"
  aria-atomic="false"
>
  {#each toasts as t (t.id)}
    <div
      class="w-72 rounded-lg border shadow-lg p-3 text-sm {t.kind === 'success'
        ? 'bg-success-bg border-success text-success'
        : t.kind === 'warning'
          ? 'bg-warning-bg border-warning-text text-warning-text'
          : t.kind === 'info'
            ? 'bg-accent-soft border-accent text-accent'
            : 'bg-surface border-border-emphasis text-primary'}"
      data-testid={`toast-${t.kind}`}
    >
      <div class="flex items-start gap-2">
        <span class="min-w-0 flex-1 break-words font-medium">{t.title}</span>
        <CloseButton
          onClick={() => {
            t.onDismiss?.();
            dismiss(t.id);
          }}
          ariaLabel={dismissLabel}
        />
      </div>
      {#if t.lines.length > 0}
        <ul class="mt-1 space-y-0.5 text-xs selectable">
          {#each t.lines as line}
            <!-- break-words (overflow-wrap) so long detail lines wrap to the
                 next line instead of being clipped to a single ellipsised row;
                 also breaks unbreakable strings like file paths / URLs. -->
            <li class="break-words">{line}</li>
          {/each}
        </ul>
      {/if}
      {#if t.action}
        <button
          type="button"
          class="btn-primary btn-sm mt-2"
          data-testid="toast-action"
          onclick={() => {
            t.action?.run();
            dismiss(t.id);
          }}
        >
          {t.action.label}
        </button>
      {/if}
    </div>
  {/each}
</div>
