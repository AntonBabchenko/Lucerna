import { expect } from 'vitest';

/**
 * The text an element is described by: every id in its `aria-describedby`,
 * resolved and joined. Fails on a missing attribute and on a dangling id — a
 * description that names an element which is not in the DOM is announced as
 * nothing, silently, which is the bug this helper exists to catch.
 */
export function describedText(el: Element): string {
  const ids = el.getAttribute('aria-describedby');
  expect(ids, `${el.tagName.toLowerCase()} has aria-describedby`).toBeTruthy();
  return (ids ?? '')
    .split(/\s+/)
    .filter(Boolean)
    .map((id) => {
      const node = document.getElementById(id);
      expect(node, `#${id} (named in aria-describedby) is in the document`).not.toBeNull();
      return node?.textContent?.trim() ?? '';
    })
    .join(' ');
}
