// The selection model behind every bulk bar (installed mods, add-ons, data packs, a world's
// packs, server mods / plugins / packs). One rule set, written once:
//   * `selected` is a Set of row keys, always reassigned whole — never mutated in place;
//   * «Select all» spans the VISIBLE set (the filtered rows, not the page);
//   * a key whose row leaves the visible set (filter / search change) is dropped;
//   * the selection is emptied when the list's scope changes (another profile, server, kind or
//     world) — the two effects are order-safe: the clear runs last, so a switch always wins.
// The effects live in $effect.root so the factory can be created in a unit test and torn down
// via dispose() on unmount; without a reactive runtime they stay inert.
export type ListSelection = ReturnType<typeof createListSelection>;

export function createListSelection(
  getVisibleKeys: () => readonly string[],
  getScopeKey: () => string | null,
) {
  let selected = $state<Set<string>>(new Set());

  const allSelected = $derived.by(() => {
    const keys = getVisibleKeys();
    return keys.length > 0 && keys.every((k) => selected.has(k));
  });
  const indeterminate = $derived(selected.size > 0 && !allSelected);

  // Drop selections for rows no longer visible. Also the body of the prune effect, exposed so
  // the rule is testable without a reactive runtime.
  function pruneToVisible(): void {
    const visible = new Set(getVisibleKeys());
    const next = new Set([...selected].filter((k) => visible.has(k)));
    if (next.size !== selected.size) selected = next;
  }

  let stopEffects: (() => void) | null = null;
  try {
    stopEffects = $effect.root(() => {
      $effect(() => {
        pruneToVisible();
      });
      $effect(() => {
        // Clear on scope switch. Keep this effect last; do not reorder (see the header).
        void getScopeKey();
        selected = new Set();
      });
    });
  } catch {
    /* no reactive runtime to root the effects in — they stay inert. Under vitest the runtime IS
       there: the effects make their first run (the scope-switch clear) at a test's first await,
       so a test that selects must let them run first. */
  }

  function toggle(key: string, checked: boolean): void {
    const next = new Set(selected);
    if (checked) next.add(key);
    else next.delete(key);
    selected = next;
  }
  function toggleAll(checked: boolean): void {
    selected = checked ? new Set(getVisibleKeys()) : new Set();
  }
  function clear(): void {
    selected = new Set();
  }

  return {
    get selected() {
      return selected;
    },
    get count() {
      return selected.size;
    },
    get allSelected() {
      return allSelected;
    },
    get indeterminate() {
      return indeterminate;
    },
    toggle,
    toggleAll,
    clear,
    pruneToVisible,
    dispose() {
      stopEffects?.();
    },
  };
}
