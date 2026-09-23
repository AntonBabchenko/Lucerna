// UPD-11. ToastHost.svelte:40-46 pairs a title text utility with a box
// background per tone: info = text-accent on bg-accent-soft, success =
// text-success on bg-success-bg, warning = text-warning-text on bg-warning-bg
// (the border is a line, not text — not a pair). Each pair must clear WCAG AA
// for normal text (4.5:1) in both themes.
//
// The triples come from src/app.css and the utility → token mapping from
// tailwind.config.cjs, the way tests/intent/design-tokens.test.ts reads the
// danger split: `text-X` resolves through `textColor.X` when that map names X
// and through `colors.X` otherwise (Tailwind's own resolution order). Before
// the accent/success split, text-accent and text-success resolve to the FILL
// tokens — which is why info-dark, success-light and success-dark fail today.
// After it they resolve to --accent-text / --success-text, and only this
// mapping — not the test — changes.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const appCss = readFileSync(resolve(process.cwd(), 'src/app.css'), 'utf8');
const tailwindConfig = readFileSync(resolve(process.cwd(), 'tailwind.config.cjs'), 'utf8');

/** WCAG 2.1 AA floor for normal-size text. */
const AA_NORMAL_TEXT = 4.5;

/** Both files carry prose comments that are free to mention a brace. */
function withoutComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/[^\n]*/g, '');
}

/** The body of the block `anchor` opens, bounded by its own closing brace. */
function block(source: string, anchor: RegExp, label: string): string {
  const match = anchor.exec(withoutComments(source));
  if (match === null) throw new Error(`no ${label} block: ${anchor} matched nothing`);
  return match[1];
}

type Rgb = readonly [number, number, number];

/** The bare `R G B` triple a token is declared as; throws when it is missing. */
function token(themeBlock: string, name: string, theme: string): Rgb {
  const match = new RegExp(`--${name}:\\s*(\\d+)\\s+(\\d+)\\s+(\\d+)`).exec(themeBlock);
  if (match === null) throw new Error(`--${name} is not declared in the ${theme} theme block`);
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

/** The custom property a `text-<name>` / `bg-<name>` utility resolves to. */
function tokenBehind(utility: 'text' | 'bg', name: string): string {
  const colors = block(tailwindConfig, /colors:\s*\{([^}]*)\}/, 'colors');
  const textColor = block(tailwindConfig, /textColor:\s*\{([^}]*)\}/, 'textColor');
  // `accent:` is bare, `'accent-soft':` is quoted; the `'?…'?:` tolerates both
  // and the leading whitespace class keeps `accent` from matching `accent-soft`.
  const entry = new RegExp(`(?:^|\\s)'?${name}'?:\\s*'rgb\\(var\\(--([\\w-]+)\\)`, 'm');
  const match = (utility === 'text' ? entry.exec(textColor) : null) ?? entry.exec(colors);
  if (match === null) throw new Error(`${utility}-${name} is not a registered utility`);
  return match[1];
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

// The class → utility pairs, hardcoded from ToastHost.svelte:40-46.
const PAIRS = [
  { tone: 'info', text: 'accent', bg: 'accent-soft' },
  { tone: 'success', text: 'success', bg: 'success-bg' },
  { tone: 'warning', text: 'warning-text', bg: 'warning-bg' },
] as const;

const THEMES = [
  { theme: 'light', anchor: /:root\s*\{([^}]*)\}/ },
  { theme: 'dark', anchor: /\.dark\s*\{([^}]*)\}/ },
] as const;

describe('toast title contrast (ToastHost.svelte tone classes)', () => {
  for (const { theme, anchor } of THEMES) {
    it.each(PAIRS)(`$tone toast title clears AA on its box in the ${theme} theme`, (pair) => {
      const themeBlock = block(appCss, anchor, theme);
      const fg = token(themeBlock, tokenBehind('text', pair.text), theme);
      const bg = token(themeBlock, tokenBehind('bg', pair.bg), theme);
      const ratio = contrastRatio(fg, bg);
      expect(
        ratio,
        `text-${pair.text} (--${tokenBehind('text', pair.text)}) on bg-${pair.bg} = ${ratio.toFixed(2)}:1`,
      ).toBeGreaterThanOrEqual(AA_NORMAL_TEXT);
    });
  }
});
