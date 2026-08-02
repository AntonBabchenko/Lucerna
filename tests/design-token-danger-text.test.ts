// Pins the --danger-text / --danger split. The two files are edited
// independently — a token can be added to app.css without Tailwind ever
// resolving `text-danger` to it, and the Tailwind map can be re-pointed at
// colors.danger by a future cleanup that sees the duplication and "tidies"
// it. Neither mistake shows up in typecheck, lint, or any component test,
// because both spellings produce a valid red. So the contract is asserted
// on the source files directly, the way tests/intent/browser-feel.test.ts
// asserts the CSS rules happy-dom cannot compute.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const appCss = readFileSync(resolve(process.cwd(), 'src/app.css'), 'utf8');
const tailwindConfig = readFileSync(resolve(process.cwd(), 'tailwind.config.cjs'), 'utf8');

/** Both files are heavily commented with rationale, and a comment is free to
 *  mention a brace (`@media (…) { … }`). Stripping comments before slicing
 *  keeps prose from reframing a block: `[^}]*` below would otherwise stop at
 *  the first brace *inside* a comment and report a correctly-defined token as
 *  missing. Handles both syntaxes because one of the two files is CSS and the
 *  other JS; neither carries a `//` outside a comment for this to trip on. */
function withoutComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/[^\n]*/g, '');
}

/**
 * Declarations inside the block `anchor` opens, bounded by its own closing
 * brace rather than by the next rule — a slice that ran to the following
 * selector would also swallow anything inserted between the two, letting a
 * `--danger-text` defined in some unrelated rule satisfy an assertion the
 * real theme block no longer does.
 */
function declarations(source: string, anchor: RegExp, label: string): string {
  const match = anchor.exec(withoutComments(source));
  if (match === null) {
    throw new Error(
      `no ${label} block: ${anchor} matched nothing — was the rule renamed or reshaped?`,
    );
  }
  return match[1];
}

/** Body of the `:root { … }` block — the light theme. */
function lightThemeBlock(): string {
  return declarations(appCss, /:root\s*\{([^}]*)\}/, ':root');
}

/** Body of the `.dark { … }` block — the dark theme. */
function darkThemeBlock(): string {
  return declarations(appCss, /\.dark\s*\{([^}]*)\}/, '.dark');
}

describe('--danger-text token', () => {
  it('is defined in the light theme', () => {
    expect(lightThemeBlock()).toMatch(/--danger-text:\s*220 38 38/);
  });

  it('is defined in the dark theme at the AA-safe red-400 tier', () => {
    // red-500 (239 68 68) is ~4.0:1 on --bg-surface and fails AA for normal
    // text; red-400 clears it at ~5.5:1.
    expect(darkThemeBlock()).toMatch(/--danger-text:\s*248 113 113/);
  });

  it('keeps --danger itself at the saturated fill tier in both themes', () => {
    // The solid .btn-danger fill needs the saturated red to keep its white
    // label readable, so the split must not drift into a single token.
    expect(lightThemeBlock()).toMatch(/--danger:\s*220 38 38/);
    expect(darkThemeBlock()).toMatch(/--danger:\s*239 68 68/);
  });

  it('is what the text-danger utility resolves to', () => {
    const textColorMap = declarations(tailwindConfig, /textColor:\s*\{([^}]*)\}/, 'textColor');
    expect(textColorMap).toMatch(/danger:\s*'rgb\(var\(--danger-text\)/);
  });
});
