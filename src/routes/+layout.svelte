<script lang="ts">
  import '../app.css';
  import { TooltipLayer } from '$lib/ui/tooltip';
  import { iconZoomFx } from '$lib/fx/icon-zoom-fx.svelte';
  import { t } from '$lib/i18n';
  import Menu from '$lib/ui/Menu.svelte';
  import type { ContextMenuItem } from '$lib/ui/cards/ContextMenu.svelte';
  import { commands } from '$lib/ipc/bindings';
  import {
    type EditAction,
    editMenuEnabled,
    isTextField,
    type SelectionRange,
    selectionRangeOf,
  } from '$lib/ui/edit-menu';
  let { children } = $props();

  // Gate the decorative icon hover-zoom at the document root so a single
  // class toggle drives every .btn-icon / .btn-icon-sm. Frontend-only,
  // mirrors the rainbowFx preference (which is gated per-icon in the
  // Sidebar). prefers-reduced-motion zeroes the transform in app.css, so
  // reduced-motion users never see it regardless of this class.
  $effect(() => {
    document.documentElement.classList.toggle('fx-icon-zoom', iconZoomFx.enabled);
  });

  // Block WebView2's native right-click menu (Back / Refresh / Save-as /
  // Print / Inspect). It betrays the browser nature of the app and shows
  // entries that make no sense in a Minecraft launcher. F12 and
  // Ctrl+Shift+I still open devtools in `pnpm tauri dev`.
  //
  // Text fields used to be the exception — they kept the native menu so that
  // right-click → Paste worked in paste-heavy fields like the CurseForge API
  // key entry. They now get the app's own menu with those same three actions
  // instead: the native surface ignores the theme (it stays light in dark
  // mode) and carries Reload / Back / spell-check, which is the same reason
  // DESIGN.md §6 bans the native select element. (Spelled out rather than as
  // a tag: tests/no-native-select.test.ts greps every .svelte file for the
  // literal element and exempts only Select.svelte by filename, so writing it
  // as a tag here — even in a comment — turns this file into an offender.)
  const MENU_WIDTH = 180;

  let menuOpen = $state(false);
  let menuTop = $state(0);
  let menuLeft = $state(0);
  // The field the menu was opened over, and the selection it had at that
  // moment. Both are needed at invoke time: Menu.svelte focuses itself on
  // open and document.execCommand acts on the FOCUSED element, so without
  // restoring these every item would be a silent no-op.
  let editField = $state<HTMLInputElement | HTMLTextAreaElement | null>(null);
  let editRange = $state<SelectionRange | null>(null);

  function onContextMenu(e: MouseEvent) {
    // e.target may be Window (event dispatched directly on window) or any
    // non-Element node, so guard before calling closest().
    const target = e.target;
    const field = target instanceof HTMLElement ? target.closest('input, textarea') : null;
    // Suppressed either way — the only question is whether the app's own menu
    // takes the native one's place.
    e.preventDefault();
    if (!isTextField(field)) return;
    editField = field;
    editRange = selectionRangeOf(field);
    menuLeft = e.clientX;
    menuTop = e.clientY;
    menuOpen = true;
  }

  async function readClipboard(): Promise<string | null> {
    const res = await commands.clipboardReadText();
    return res.status === 'ok' && res.data !== '' ? res.data : null;
  }

  async function runEdit(action: EditAction) {
    const field = editField;
    const range = editRange;
    // Close first so the teardown is ours and ordered, rather than racing the
    // await below.
    menuOpen = false;
    if (!field) return;
    // Read the clipboard BEFORE touching focus: an await between the focus
    // call and the command would let Menu's own close-and-restore run in
    // between, and execCommand would fire at the wrong element.
    const text = action === 'paste' ? await readClipboard() : null;
    if (action === 'paste' && text === null) return;
    field.focus();
    // Only when the field exposes a selection at all — setSelectionRange
    // throws on a number input for the same reason selectionRangeOf does.
    if (range) field.setSelectionRange(range.start, range.end);
    if (text !== null) {
      // insertText rather than assigning .value: it keeps the field's native
      // undo stack intact and fires `input`, without which bind:value never
      // sees the change and the model silently diverges from the screen.
      document.execCommand('insertText', false, text);
      return;
    }
    document.execCommand(action);
  }

  const editItems = $derived.by<ContextMenuItem[]>(() => {
    const enabled = editMenuEnabled(
      editRange !== null && editRange.start !== editRange.end,
      editField?.readOnly ?? false,
    );
    return [
      {
        label: $t('common.editMenu.copy'),
        disabled: !enabled.copy,
        testId: 'edit-menu-copy',
        onSelect: () => void runEdit('copy'),
      },
      {
        label: $t('common.editMenu.cut'),
        disabled: !enabled.cut,
        testId: 'edit-menu-cut',
        onSelect: () => void runEdit('cut'),
      },
      {
        label: $t('common.editMenu.paste'),
        disabled: !enabled.paste,
        testId: 'edit-menu-paste',
        onSelect: () => void runEdit('paste'),
      },
    ];
  });
</script>

<svelte:window oncontextmenu={onContextMenu} />

{@render children?.()}

{#if menuOpen}
  <Menu
    items={editItems}
    ariaLabel={$t('common.editMenu.ariaLabel')}
    top={menuTop}
    left={menuLeft}
    width={MENU_WIDTH}
    onClose={() => (menuOpen = false)}
  />
{/if}

<TooltipLayer />
