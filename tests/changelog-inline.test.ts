import { describe, expect, it } from 'vitest';
import { parseInline } from '$lib/changelog/inline';

describe('parseInline', () => {
  it('returns a single plain-text segment for unformatted text', () => {
    expect(parseInline('just text')).toEqual([{ value: 'just text', bold: false, code: false }]);
  });

  it('parses a leading bold run followed by text', () => {
    expect(parseInline('**Add-ons browser.** rest of it')).toEqual([
      { value: 'Add-ons browser.', bold: true, code: false },
      { value: ' rest of it', bold: false, code: false },
    ]);
  });

  it('parses a code span', () => {
    expect(parseInline('back up `.mrpack` files')).toEqual([
      { value: 'back up ', bold: false, code: false },
      { value: '.mrpack', bold: false, code: true },
      { value: ' files', bold: false, code: false },
    ]);
  });

  it('parses a code span nested inside a bold run', () => {
    expect(parseInline('**Renamed `FTlauncher` now.**')).toEqual([
      { value: 'Renamed ', bold: true, code: false },
      { value: 'FTlauncher', bold: true, code: true },
      { value: ' now.', bold: true, code: false },
    ]);
  });

  it('decodes HTML entities in text', () => {
    expect(parseInline('Open on &lt;platform&gt;')).toEqual([
      { value: 'Open on <platform>', bold: false, code: false },
    ]);
  });

  it('decodes &amp; last so &amp;lt; does not become <', () => {
    expect(parseInline('a &amp;lt; b')).toEqual([{ value: 'a &lt; b', bold: false, code: false }]);
  });

  it('leaves an unmatched ** as literal text', () => {
    expect(parseInline('2 ** 3 is power')).toEqual([
      { value: '2 ** 3 is power', bold: false, code: false },
    ]);
  });

  it('leaves an unmatched backtick as literal text', () => {
    expect(parseInline('a ` b')).toEqual([{ value: 'a ` b', bold: false, code: false }]);
  });
});
