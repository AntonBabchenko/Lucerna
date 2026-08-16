// tests/test-utils/reveal-tooltip.ts
// Drive a `use:tooltip` trigger to its visible state, synchronously.
//
// The action has two open paths and they are NOT equivalent:
//   mouseenter → open(false) → showTooltip schedules the reveal OPEN_DELAY_MS
//                (400ms) later, so an assertion on the next line reads the
//                pre-open state and every such test passes for the wrong reason;
//   focusin    → open(true)  → reveals immediately.
// Focus is also the path a DESIGN.md §5 fix is actually about: a native
// `title=` is unreachable by keyboard, and routing the string through
// `use:tooltip` is what makes it reachable. So these helpers drive focus, and
// no fake timers are involved.
//
// `use:tooltip` gates focus on `:focus-visible`, so a modal's programmatic
// focus-restore cannot pop a spurious bubble. happy-dom cannot model focus
// modality — tests/tooltip/tooltip-action.test.ts stubs `node.matches` for
// exactly that reason — so these helpers stub it the same way, on the node they
// are handed. The stub is per-node and the node is discarded by the per-test
// DOM cleanup, so nothing leaks between cases.
//
// Deliberately dependency-free. Importing `vitest` here would make this a
// "file containing tests" for Biome's lint/suspicious/noExportsInTest, which
// bans exports from such files — and the whole point of this module is to be
// imported.

/** Focus `node` as a keyboard user would, so its tooltip opens immediately. */
export function revealTooltip(node: Element): void {
  node.matches = () => true;
  node.dispatchEvent(new FocusEvent('focusin'));
}

/** Blur counterpart — the action's focusout path hides without delay. */
export function dismissTooltip(node: Element): void {
  node.dispatchEvent(new FocusEvent('focusout'));
}
