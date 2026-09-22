// A deep link (fieldFlash with focus) may place focus inside a dialog before
// the dialog's own deferred initial focus runs. That initial focus must not
// take it away — otherwise "the key field takes focus" is a lie on every
// banner path, and the search box wins.
import { afterEach, describe, expect, it } from 'vitest';

import { trapFocus } from '$lib/ui/trap-focus';

let cleanup: (() => void) | null = null;

afterEach(() => {
  cleanup?.();
  cleanup = null;
  document.body.innerHTML = '';
});

const frame = (): Promise<void> => new Promise((r) => requestAnimationFrame(() => r()));

function mountDialog(): {
  dialog: HTMLElement;
  search: HTMLInputElement;
  other: HTMLButtonElement;
} {
  const dialog = document.createElement('div');
  dialog.tabIndex = -1;
  const search = document.createElement('input');
  search.setAttribute('data-autofocus', '');
  const other = document.createElement('button');
  other.textContent = 'Save key';
  dialog.append(search, other);
  document.body.appendChild(dialog);
  return { dialog, search, other };
}

describe('trapFocus and a focus already placed inside the dialog', () => {
  it('leaves focus alone when something inside the dialog already has it', async () => {
    const { dialog, other } = mountDialog();
    other.focus();
    const handle = trapFocus(dialog);
    cleanup = () => handle.destroy();
    await frame();
    expect(document.activeElement).toBe(other);
  });

  it('still takes initial focus when focus is outside the dialog', async () => {
    const outside = document.createElement('button');
    document.body.appendChild(outside);
    outside.focus();
    const { dialog, search } = mountDialog();
    const handle = trapFocus(dialog);
    cleanup = () => handle.destroy();
    await frame();
    expect(document.activeElement).toBe(search);
  });
});
