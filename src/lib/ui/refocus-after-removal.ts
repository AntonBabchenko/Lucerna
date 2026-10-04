// After a removal the control that had focus is gone with its row — the row's Remove, a dialog
// that returned focus to it, the bulk bar's Remove that left with the selection — and focus fell
// to <body>. It goes to the row now in that place: the next one, else the one before it, else
// the list's own first control (its select-all), else the empty list, which says there is nothing
// left. Never pulled from wherever the user went meanwhile, and not at all after a cancel (focus
// came back to the control that asked). DESIGN.md §13 «Focus survives a removal».
import { tick } from 'svelte';

export async function refocusAfterRemoval(opts: {
  /** The index of the removed row (the first of them for a batch) in the rows as rendered. */
  index: number;
  listEl: HTMLElement | null;
  /** The row elements of the list, in render order. */
  rows: (list: HTMLElement) => HTMLElement[];
  /** The element the empty list renders (tabindex -1), when the last row went. */
  emptyEl?: HTMLElement | null;
}): Promise<void> {
  await tick();
  if (typeof document === 'undefined') return;
  const active = document.activeElement;
  if (active !== null && active !== document.body && active.isConnected) return;
  const rows = opts.listEl ? opts.rows(opts.listEl) : [];
  const row = rows[Math.min(opts.index, rows.length - 1)];
  // A row's first control (its checkbox), else the list's own first (select all), else the
  // empty list.
  const into = (el: HTMLElement | null | undefined) =>
    el?.querySelector<HTMLElement>('input, button') ?? null;
  (into(row) ?? into(opts.listEl) ?? opts.emptyEl ?? null)?.focus();
}
