<script lang="ts">
  import { dismiss, toastList } from '$lib/toasts/toasts.svelte';

  // Renders the active toast stack in the top-right corner. Mounted once
  // at the app root (`src/routes/+page.svelte`). z-50 keeps completion
  // toasts above the import progress toast (ImportProgressView, z-40).
  const toasts = $derived(toastList());
</script>

<div class="fixed top-4 right-4 z-50 flex flex-col gap-2" data-testid="toast-host">
  {#each toasts as t (t.id)}
    <div
      class="w-72 rounded-lg border shadow-lg p-3 text-sm"
      class:bg-green-50={t.kind === 'success'}
      class:border-green-200={t.kind === 'success'}
      class:text-green-900={t.kind === 'success'}
      class:bg-amber-50={t.kind === 'warning'}
      class:border-amber-200={t.kind === 'warning'}
      class:text-amber-900={t.kind === 'warning'}
      role="status"
      data-testid={`toast-${t.kind}`}
    >
      <div class="flex items-start gap-2">
        <span class="flex-1 font-medium">{t.title}</span>
        {#if t.kind === 'warning'}
          <button
            type="button"
            class="leading-none opacity-60 hover:opacity-100"
            aria-label="Dismiss"
            onclick={() => dismiss(t.id)}
          >
            ×
          </button>
        {/if}
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
