<script lang="ts">
  import { dismiss, toastList } from '$lib/toasts/toasts.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';

  // Renders the active toast stack in the top-right corner. Mounted once
  // at the app root (`src/routes/+page.svelte`). z-[200] keeps toasts
  // above EVERYTHING — modals (z-40-50), contextual tour overlays
  // (z-100/101), drag-drop dropzone highlights. Toasts are the only
  // chrome that announces transient state; they must never be obscured.
  const toasts = $derived(toastList());
</script>

<div class="fixed top-4 right-4 z-[200] flex flex-col gap-2" data-testid="toast-host">
  {#each toasts as t (t.id)}
    <div
      class="w-72 rounded-lg border shadow-lg p-3 text-sm bg-surface {t.kind === 'success'
        ? 'border-success text-success'
        : t.kind === 'warning'
          ? 'border-warning-text text-warning-text'
          : t.kind === 'info'
            ? 'border-accent text-accent'
            : 'border-border-emphasis text-primary'}"
      role="status"
      data-testid={`toast-${t.kind}`}
    >
      <div class="flex items-start gap-2">
        <span class="flex-1 font-medium">{t.title}</span>
        <CloseButton onClick={() => dismiss(t.id)} ariaLabel="Dismiss notification" />
      </div>
      {#if t.lines.length > 0}
        <ul class="mt-1 space-y-0.5 text-xs">
          {#each t.lines as line}
            <li class="truncate">{line}</li>
          {/each}
        </ul>
      {/if}
    </div>
  {/each}
</div>
