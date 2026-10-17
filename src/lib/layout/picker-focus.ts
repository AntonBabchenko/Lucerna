// Where the keyboard focus goes when the control that should get it back is gone.
//
// A dialog the sidebar's row menu opened has no opener to restore the focus to:
// the menu item that opened it left the page with the menu, so `trapFocus` has
// nothing to give it back to and the browser drops it to <body> — the next Tab
// starts again from the top of the window. The instance picker names the
// instance the dialog was about, so it is where the focus goes (spec #510 §9;
// DESIGN.md §13). A dialog opened from a button on the page restores to that
// button, and this then does nothing.
import { tick } from 'svelte';

const PICKER = '[data-tour="instance-picker"] [role="combobox"]';

/** Focus the sidebar's instance picker. */
export function focusInstancePicker(): void {
  document.querySelector<HTMLElement>(PICKER)?.focus();
}

/** After a dialog closes: focus the instance picker only if the focus was lost. */
export async function focusInstancePickerIfLost(): Promise<void> {
  await tick();
  const active = document.activeElement;
  if (active === null || active === document.body) focusInstancePicker();
}
