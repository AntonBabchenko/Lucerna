// Pure logic for the right-click edit menu on text fields. No DOM mutation and
// no IPC — the layout owns those; this only answers "is this a text field",
// "what is selected in it" and "which of the three items should be live".

/** Input types that carry text the user could copy, cut into or paste over.
 *  Deliberately an allowlist: `button` / `checkbox` / `radio` / `submit` /
 *  `range` / `color` / `file` all exist in this app and none of them has text,
 *  so an inferred "is it not a button" rule would offer a meaningless menu on
 *  a colour swatch. A bare <input> with no type attribute defaults to text. */
const TEXT_INPUT_TYPES: ReadonlySet<string> = new Set([
  'text',
  'search',
  'password',
  'number',
  'email',
  'url',
  'tel',
]);

export function isTextField(el: Element | null): el is HTMLInputElement | HTMLTextAreaElement {
  if (el instanceof HTMLTextAreaElement) return true;
  if (!(el instanceof HTMLInputElement)) return false;
  return TEXT_INPUT_TYPES.has(el.type);
}

export type SelectionRange = { start: number; end: number };

/** The field's selection, or null when it will not expose one.
 *
 *  Reading `selectionStart` on `type="number"` throws `InvalidStateError` in
 *  Chromium and returns null in jsdom. Nine of this app's inputs are number
 *  inputs (memory, ports), so an unguarded read would throw on exactly the
 *  fields where pasting a value is most useful. Both outcomes collapse to the
 *  same answer: there is no range, so there is nothing to restore and nothing
 *  to copy — but Paste still works, because inserting at the caret does not
 *  need the selection API. */
export function selectionRangeOf(
  el: HTMLInputElement | HTMLTextAreaElement,
): SelectionRange | null {
  try {
    const start = el.selectionStart;
    const end = el.selectionEnd;
    if (start === null || end === null) return null;
    return { start, end };
  } catch {
    return null;
  }
}

export type EditAction = 'copy' | 'cut' | 'paste';

/** Which items are live. Copy is a read, so it survives `readOnly`; cut and
 *  paste write, so they do not. */
export function editMenuEnabled(
  hasSelection: boolean,
  readOnly: boolean,
): Record<EditAction, boolean> {
  return {
    copy: hasSelection,
    cut: hasSelection && !readOnly,
    paste: !readOnly,
  };
}
