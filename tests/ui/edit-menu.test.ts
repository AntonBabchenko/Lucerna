import { describe, expect, it } from 'vitest';
import { editMenuEnabled, isTextField, selectionRangeOf } from '$lib/ui/edit-menu';

function input(type?: string): HTMLInputElement {
  const el = document.createElement('input');
  if (type) el.type = type;
  return el;
}

describe('editMenuEnabled', () => {
  it('offers copy and cut only when something is selected', () => {
    expect(editMenuEnabled(true, false)).toEqual({ copy: true, cut: true, paste: true });
    expect(editMenuEnabled(false, false)).toEqual({ copy: false, cut: false, paste: true });
  });

  // Copy is a read: it stays available on a field the user cannot change.
  it('keeps copy but drops cut and paste on a read-only field', () => {
    expect(editMenuEnabled(true, true)).toEqual({ copy: true, cut: false, paste: false });
    expect(editMenuEnabled(false, true)).toEqual({ copy: false, cut: false, paste: false });
  });
});

describe('isTextField', () => {
  it('accepts the text-bearing input types the app actually uses', () => {
    for (const type of ['text', 'search', 'password', 'number']) {
      expect(isTextField(input(type))).toBe(true);
    }
    // A bare <input> defaults to text.
    expect(isTextField(input())).toBe(true);
    expect(isTextField(document.createElement('textarea'))).toBe(true);
  });

  it('rejects controls that carry no text', () => {
    for (const type of ['button', 'checkbox', 'radio', 'submit', 'range', 'color', 'file']) {
      expect(isTextField(input(type))).toBe(false);
    }
    expect(isTextField(document.createElement('div'))).toBe(false);
    expect(isTextField(null)).toBe(false);
  });
});

describe('selectionRangeOf', () => {
  it('reads the caret range from a text input', () => {
    const el = input('text');
    el.value = 'hello';
    el.setSelectionRange(1, 4);
    expect(selectionRangeOf(el)).toEqual({ start: 1, end: 4 });
  });

  // Chromium throws InvalidStateError when the selection API is read on a
  // number input; jsdom returns null instead. Both mean the same thing here —
  // there is no range to restore — and both must be survivable, because nine
  // of the app's inputs are type="number".
  it('returns null when the element refuses to expose a selection', () => {
    const throwing = {
      get selectionStart(): number | null {
        throw new DOMException('unsupported', 'InvalidStateError');
      },
      get selectionEnd(): number | null {
        return null;
      },
    } as unknown as HTMLInputElement;
    expect(selectionRangeOf(throwing)).toBeNull();

    const nulling = { selectionStart: null, selectionEnd: null } as unknown as HTMLInputElement;
    expect(selectionRangeOf(nulling)).toBeNull();
  });
});
