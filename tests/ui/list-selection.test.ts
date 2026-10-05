// The one selection model behind every bulk bar: a Set of keys over the VISIBLE (filtered)
// rows, pruned when a row leaves the visible set, emptied when the list's scope changes.
import { describe, expect, it } from 'vitest';
import { createListSelection } from '$lib/ui/list-selection.svelte';

const tick = () => new Promise<void>((r) => setTimeout(r, 0));

describe('createListSelection', () => {
  it('toggles one key and reports allSelected / indeterminate over the visible set', async () => {
    const sel = createListSelection(
      () => ['a', 'b', 'c'],
      () => 'scope-1',
    );
    await tick(); // the effects' first run (the scope-switch clear) happens here
    sel.toggle('a', true);
    expect([...sel.selected]).toEqual(['a']);
    expect(sel.count).toBe(1);
    expect(sel.allSelected).toBe(false);
    expect(sel.indeterminate).toBe(true);
    sel.toggle('b', true);
    sel.toggle('c', true);
    expect(sel.allSelected).toBe(true);
    expect(sel.indeterminate).toBe(false);
    sel.toggle('a', false);
    expect(sel.allSelected).toBe(false);
    sel.dispose();
  });

  it('select all takes every visible key; unchecking clears', async () => {
    const sel = createListSelection(
      () => ['a', 'b'],
      () => 'scope-1',
    );
    await tick();
    sel.toggleAll(true);
    expect([...sel.selected].sort()).toEqual(['a', 'b']);
    sel.toggleAll(false);
    expect(sel.count).toBe(0);
    sel.dispose();
  });

  it('an empty visible set is never "all selected"', async () => {
    const sel = createListSelection(
      () => [],
      () => 'scope-1',
    );
    await tick();
    expect(sel.allSelected).toBe(false);
    sel.dispose();
  });

  it('drops a selected key that leaves the visible set (filter change)', async () => {
    let visible = ['a', 'b'];
    const sel = createListSelection(
      () => visible,
      () => 'scope-1',
    );
    await tick();
    sel.toggleAll(true);
    expect(sel.count).toBe(2);
    visible = ['b'];
    // The composable reads plain closures, not signals: in the app the effect re-runs on the
    // caller's own signals; here the test pokes the composable the way its effect would.
    sel.pruneToVisible();
    expect([...sel.selected]).toEqual(['b']);
    sel.dispose();
  });

  it('clear() empties the selection', async () => {
    const sel = createListSelection(
      () => ['a'],
      () => 'scope-1',
    );
    await tick();
    sel.toggle('a', true);
    sel.clear();
    expect(sel.count).toBe(0);
    sel.dispose();
  });
});
