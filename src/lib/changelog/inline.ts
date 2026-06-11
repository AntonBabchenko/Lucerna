// Minimal inline-Markdown renderer for changelog entry bodies. The parser
// (parse.ts) keeps bullet text verbatim, which left literal `**bold**`,
// `` `code` `` and HTML entities (e.g. `&lt;`) showing through in the UI. This
// turns a line into a flat list of styled segments the panel renders with
// plain Svelte markup (no `{@html}`), so the output is escaped by construction.
//
// Scope is deliberately small — the only inline constructs the project's
// CHANGELOG.md uses inside bullets are `**bold**`, `` `code` `` (which may sit
// inside a bold run), and a handful of HTML entities. No links, no nesting
// beyond bold-containing-code.

export interface InlineSegment {
  value: string;
  bold: boolean;
  code: boolean;
}

// Decode the small set of HTML entities the changelog actually contains.
// `&amp;` is decoded last so an input like `&amp;lt;` becomes `&lt;`, not `<`.
function decodeEntities(s: string): string {
  return s
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&amp;/g, '&');
}

function scan(input: string, bold: boolean, out: InlineSegment[]): void {
  let i = 0;
  let text = '';
  const flush = (): void => {
    if (text) {
      out.push({ value: decodeEntities(text), bold, code: false });
      text = '';
    }
  };

  while (i < input.length) {
    // Code span `...` — literal content, may appear inside a bold run.
    if (input[i] === '`') {
      const end = input.indexOf('`', i + 1);
      if (end > i) {
        flush();
        out.push({ value: decodeEntities(input.slice(i + 1, end)), bold, code: true });
        i = end + 1; // advance past the closing backtick
        continue;
      }
    }
    // Bold **...** — only at the top level (no nested bold in this changelog).
    if (!bold && input[i] === '*' && input[i + 1] === '*') {
      const end = input.indexOf('**', i + 2);
      if (end > i + 1) {
        flush();
        scan(input.slice(i + 2, end), true, out);
        i = end + 2;
        continue;
      }
    }
    text += input[i];
    i += 1;
  }
  flush();
}

/**
 * Split a changelog bullet's text into styled segments. Unmatched `**` or
 * `` ` `` are left as literal text (tolerant). HTML entities are decoded.
 */
export function parseInline(input: string): InlineSegment[] {
  const out: InlineSegment[] = [];
  scan(input, false, out);
  return out;
}
