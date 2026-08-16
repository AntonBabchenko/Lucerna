// Pins the --danger-text / --danger split. The two files are edited
// independently — a token can be added to app.css without Tailwind ever
// resolving `text-danger` to it, and the Tailwind map can be re-pointed at
// colors.danger by a future cleanup that sees the duplication and "tidies"
// it. Neither mistake shows up in typecheck, lint, or any component test,
// because both spellings produce a valid red. So the contract is asserted
// on the source files directly, the way tests/intent/browser-feel.test.ts
// asserts the CSS rules happy-dom cannot compute.
//
// The accessibility assertions derive the contrast ratio rather than pinning
// the foreground triple, because the ratio is what the token exists for and it
// has two inputs. --bg-surface has already been retuned once (23 23 23 →
// 38 38 38, recorded in app.css), and a pinned foreground would sail through a
// future background lift that broke AA. Deriving also lets a deliberate red
// retune pass on its own merits instead of failing for a colour nobody can
// see the difference in.

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const appCss = readFileSync(resolve(process.cwd(), 'src/app.css'), 'utf8');
const tailwindConfig = readFileSync(resolve(process.cwd(), 'tailwind.config.cjs'), 'utf8');

/** WCAG 2.1 AA floor for normal-size text. */
const AA_NORMAL_TEXT = 4.5;

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

type Rgb = readonly [number, number, number];

/** The bare `R G B` triple a token is declared as. Throws rather than
 *  returning a default, so a token deleted outright fails as loudly as one
 *  retuned past the threshold. */
function token(block: string, name: string, theme: string): Rgb {
  const match = new RegExp(`--${name}:\\s*(\\d+)\\s+(\\d+)\\s+(\\d+)`).exec(block);
  if (match === null) {
    throw new Error(`--${name} is not declared in the ${theme} theme block`);
  }
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

/** WCAG 2.1 relative luminance of an sRGB colour. */
function relativeLuminance([r, g, b]: Rgb): number {
  const linear = (raw: number): number => {
    const c = raw / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
}

/** WCAG 2.1 contrast ratio between two sRGB colours, order-independent. */
function contrastRatio(a: Rgb, b: Rgb): number {
  const [lighter, darker] = [relativeLuminance(a), relativeLuminance(b)].sort((x, y) => y - x);
  return (lighter + 0.05) / (darker + 0.05);
}

describe('--danger-text token', () => {
  // Scope: --bg-surface is the pairing this token is guaranteed on — cards,
  // inputs, popovers, and the modal body. It is deliberately NOT asserted
  // against --danger-bg: text-danger inside a bg-danger-bg error box is ~4.4:1
  // light and ~3.6:1 dark and misses AA today. That gap is recorded in
  // DESIGN.md §1 rather than pinned here, because a test can only lock in a
  // contract the palette actually meets.
  it('clears AA for normal text on --bg-surface in the light theme', () => {
    const light = lightThemeBlock();
    const ratio = contrastRatio(
      token(light, 'danger-text', 'light'),
      token(light, 'bg-surface', 'light'),
    );
    expect(ratio).toBeGreaterThanOrEqual(AA_NORMAL_TEXT);
  });

  it('clears AA for normal text on --bg-surface in the dark theme', () => {
    const dark = darkThemeBlock();
    const ratio = contrastRatio(
      token(dark, 'danger-text', 'dark'),
      token(dark, 'bg-surface', 'dark'),
    );
    expect(ratio).toBeGreaterThanOrEqual(AA_NORMAL_TEXT);
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

describe('tour spotlight motion (§12)', () => {
  // §12: "Every animated property is `transform`, `opacity`, or `color` — never
  // layout-bound props (width/height/top/left/margin/padding/border/font-size)."
  // The spotlight used `transition-all` over inline left/top/width/height plus a
  // 9999px box-shadow, i.e. four layout-bound properties and a full-viewport
  // repaint on every step. Deriving the animated property list — rather than
  // pinning the rule text — is what makes this survive a duration or easing
  // retune while still failing if someone reintroduces a geometry tween.
  it('animates opacity and nothing else', () => {
    const kf = /@keyframes tour-spotlight-in\s*\{([\s\S]*?)\n\}/.exec(withoutComments(appCss));
    if (kf === null) {
      throw new Error('no @keyframes tour-spotlight-in — was the rule renamed or removed?');
    }
    const animated = [...kf[1].matchAll(/^\s*([a-z-]+)\s*:/gm)].map((m) => m[1]);
    expect(animated.length).toBeGreaterThan(0);
    expect([...new Set(animated)]).toEqual(['opacity']);
  });

  it('spends the shared duration and easing tokens rather than a literal', () => {
    const rule = /\.tour-spotlight\s*\{([^}]*)\}/.exec(withoutComments(appCss));
    if (rule === null) throw new Error('no .tour-spotlight rule');
    expect(rule[1]).toContain('var(--duration-base)');
    expect(rule[1]).toContain('var(--ease-standard)');
  });
});
