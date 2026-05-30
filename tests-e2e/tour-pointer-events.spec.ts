// Real-browser regression test for the onboarding tour pointer-events bug
// (v0.5.0 sub-5, RESOLVED 2026-05-20).
//
// Bug: the `body[data-tour-active]` rule in `src/app.css` originally used
// `*:not(.tour-overlay *):not(.tour-overlay)` to disable `pointer-events`
// on everything except the tour. But `pointer-events` is an INHERITED
// property — that rule still set `none` on every ANCESTOR of
// `.tour-overlay`, and `.tour-overlay` inherited `none` through them.
// The whole tour became non-interactive whenever `.tour-overlay` was
// nested below <body> (always, in the real app).
//
// happy-dom (the `pnpm test` environment) does not evaluate the CSS
// cascade / inheritance / hit-testing, so this regression is invisible
// there. This test drives real headless Chromium via Playwright.
//
// It loads the actual `src/app.css` (the `@tailwind` at-rules are
// harmlessly ignored by the browser as unknown at-rules; the plain
// `body[data-tour-active]` rules under test apply normally), builds a
// `.tour-overlay` nested several levels deep — exactly the condition
// that triggered the bug — and asserts the tour stays clickable while
// the background is disabled.

import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

// Load the real app CSS so the test reflects actual production styles.
const here = dirname(fileURLToPath(import.meta.url));
const appCss = readFileSync(join(here, '..', 'src', 'app.css'), 'utf8');

// `.tour-overlay` nested deep, mirroring the real app
// (body > #app > layout > cell > .tour-overlay).
const fixture = `<!DOCTYPE html>
<html>
<head><style>${appCss}</style></head>
<body data-tour-active="true">
  <p id="bg-text">background text</p>
  <button id="bg-button">background button</button>
  <div id="app"><div class="layout"><div class="cell">
    <div class="tour-overlay">
      <div class="dim" style="position:fixed;inset:0;pointer-events:none"></div>
      <div role="dialog" style="position:fixed;top:40px;left:40px">
        <button data-tour-primary id="tour-next">Next</button>
      </div>
    </div>
  </div></div></div>
</body>
</html>`;

test.describe('Onboarding tour pointer-events regression', () => {
  test('tour stays interactive while background is disabled', async ({ page }) => {
    await page.setContent(fixture);

    const probe = await page.evaluate(() => {
      const pe = (sel: string) => {
        const el = document.querySelector(sel);
        return el ? getComputedStyle(el).pointerEvents : 'MISSING';
      };
      const btn = document.getElementById('tour-next')!;
      const r = btn.getBoundingClientRect();
      const hit = document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2);
      return {
        bgText: pe('#bg-text'),
        bgButton: pe('#bg-button'),
        tourOverlay: pe('.tour-overlay'),
        tourDim: pe('.dim'),
        tourButton: pe('#tour-next'),
        hitIsTourButton: hit ? btn.contains(hit) : false,
      };
    });

    // Background must be disabled while the tour is active.
    expect(probe.bgText, 'background text is non-interactive').toBe('none');
    expect(probe.bgButton, 'background button is non-interactive').toBe('none');

    // The tour itself must stay interactive despite being nested deep.
    expect(probe.tourOverlay, '.tour-overlay is interactive').toBe('auto');
    expect(probe.tourButton, 'tour primary button is interactive').toBe('auto');
    expect(probe.hitIsTourButton, 'tour primary button is hit-testable (clickable)').toBe(true);

    // The dim layer keeps its explicit pointer-events:none (clicks fall through).
    expect(probe.tourDim, 'dim layer stays non-interactive').toBe('none');
  });
});
