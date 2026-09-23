<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import { events } from '$lib/ipc/bindings';
  import {
    dismiss,
    pauseToastTimer,
    pushSuccess,
    resumeToastTimer,
    toastList,
  } from '$lib/toasts/toasts.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import { modalDepth } from '$lib/ui/Modal.svelte';

  onMount(() => {
    const un = events.gpuPrefApplied.listen((e) => {
      pushSuccess($t('settings.general.gpu.appliedToast', { name: e.payload.gpu_name ?? '' }));
    });
    return () => {
      void un.then((f) => f());
    };
  });

  // Renders the active toast stack. Mounted once at the app root
  // (`src/routes/+page.svelte`). z-[var(--z-toast)] (200) keeps toasts above
  // EVERYTHING — modals (z-40-50), contextual tour overlays (z-100/101),
  // drag-drop dropzone highlights. Toasts are the only chrome that announces
  // transient state; they must never be obscured — and must not obscure.
  //
  // Two positions. At rest the stack sits top right, where the main window
  // has no control. While any modal is open its panel is vertically centred:
  // its header (and the ×) is not at the viewport top — at 820×520 the
  // Settings header spans ~52–97 px, so a lower top offset would still cover
  // the × — and a footer holds the primary action at the bottom right. No
  // corner is free, so the stack moves to the bottom centre, newest nearest
  // the edge (flex-col-reverse): footers are justify-end / justify-between,
  // which leaves the middle clear, and a header's × is untouched.
  // A timed toast waits while someone is reading it: its countdown pauses
  // while the pointer is over it or focus is inside it, and resumes with the
  // time that was left. Listeners, not ARIA: the card is not a widget.
  function holdWhileEngaged(node: HTMLElement, id: number) {
    const enter = () => pauseToastTimer(id);
    const leave = () => resumeToastTimer(id);
    // focusin bubbles on every hop between the toast's own buttons: focus is ONE
    // reason to wait however many buttons it visits, and only leaving the toast
    // gives it back — counting each hop left the toast on screen for good.
    let focusInside = false;
    const focusIn = () => {
      if (focusInside) return;
      focusInside = true;
      pauseToastTimer(id);
    };
    const focusOut = (e: FocusEvent) => {
      if (!focusInside || node.contains(e.relatedTarget as Node | null)) return;
      focusInside = false;
      resumeToastTimer(id);
    };
    node.addEventListener('pointerenter', enter);
    node.addEventListener('pointerleave', leave);
    node.addEventListener('focusin', focusIn);
    node.addEventListener('focusout', focusOut);
    return {
      destroy() {
        node.removeEventListener('pointerenter', enter);
        node.removeEventListener('pointerleave', leave);
        node.removeEventListener('focusin', focusIn);
        node.removeEventListener('focusout', focusOut);
      },
    };
  }

  const toasts = $derived(toastList());
  const dismissLabel = $derived($t('common.dismissNotification'));
  const placement = $derived(
    modalDepth() > 0
      ? 'bottom-4 left-1/2 -translate-x-1/2 flex-col-reverse'
      : 'top-4 right-4 flex-col',
  );
</script>

<!-- The aria-live region is ALWAYS in the DOM (even with zero toasts) so a
     screen reader has it registered before the first toast fires — a region
     inserted at the same moment its first child appears can be missed. Only
     the individual toast children are conditional. `aria-atomic="false"` so
     each newly-added toast is announced on its own rather than re-reading the
     whole stack. -->
<div
  class="fixed z-[var(--z-toast)] flex gap-2 {placement}"
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
      use:holdWhileEngaged={t.id}
    >
      <div class="flex items-start gap-2">
        <span class="min-w-0 flex-1 break-words font-medium">{t.title}</span>
        <CloseButton onClick={() => dismiss(t.id)} ariaLabel={dismissLabel} />
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
      {#if t.progress !== undefined}
        <!-- Determinate when a fraction is known; a calm pulse when the
             server sent no Content-Length. Opacity/width only — compositor
             friendly. -->
        <div
          class="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-black/10"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={t.progress === null ? undefined : Math.round(t.progress * 100)}
        >
          {#if t.progress === null}
            <div class="h-full w-1/3 animate-pulse rounded-full bg-accent"></div>
          {:else}
            <div
              class="h-full rounded-full bg-accent"
              style="width: {Math.round(Math.min(1, Math.max(0, t.progress)) * 100)}%"
            ></div>
          {/if}
        </div>
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
        {#if t.secondary}
          <button
            type="button"
            class="btn-ghost btn-sm mt-2 ml-1"
            data-testid="toast-secondary-action"
            onclick={() => {
              t.secondary?.run();
              dismiss(t.id);
            }}
          >
            {t.secondary.label}
          </button>
        {/if}
      {/if}
    </div>
  {/each}
</div>
