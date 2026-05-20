// Real-browser regression test for the onboarding tour pointer-events bug.
//
// Bug (v0.5.0 sub-5): the `body[data-tour-active]` rule in `src/app.css`
// originally used `*:not(.tour-overlay *):not(.tour-overlay)` to disable
// `pointer-events` on everything except the tour. But `pointer-events`
// is an INHERITED property — that rule still set `none` on every
// ANCESTOR of `.tour-overlay`, and `.tour-overlay` inherited `none`
// through them. The whole tour became non-interactive whenever
// `.tour-overlay` was nested below <body> (always, in the real app).
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
//
// Run: pnpm test:e2e   (exit code 0 = pass, 1 = fail)

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { chromium } from 'playwright';

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

const checks = [];
function check(name, ok, detail) {
  checks.push({ name, ok });
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${ok ? '' : `  -> ${detail}`}`);
}

const browser = await chromium.launch();
try {
  const page = await browser.newPage();
  await page.setContent(fixture);

  const probe = await page.evaluate(() => {
    const pe = (sel) => {
      const el = document.querySelector(sel);
      return el ? getComputedStyle(el).pointerEvents : 'MISSING';
    };
    const btn = document.getElementById('tour-next');
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
  check('background text is non-interactive', probe.bgText === 'none', probe.bgText);
  check('background button is non-interactive', probe.bgButton === 'none', probe.bgButton);

  // The tour itself must stay interactive despite being nested deep.
  check('.tour-overlay is interactive', probe.tourOverlay === 'auto', probe.tourOverlay);
  check('tour primary button is interactive', probe.tourButton === 'auto', probe.tourButton);
  check(
    'tour primary button is hit-testable (clickable)',
    probe.hitIsTourButton === true,
    `elementFromPoint did not land on the button`,
  );

  // The dim layer keeps its explicit pointer-events:none (clicks fall through).
  check('dim layer stays non-interactive', probe.tourDim === 'none', probe.tourDim);
} finally {
  await browser.close();
}

const failed = checks.filter((c) => !c.ok).length;
console.log('');
console.log(`${checks.length - failed}/${checks.length} checks passed`);
if (failed > 0) {
  console.error('REGRESSION: onboarding tour pointer-events behaviour broken.');
  process.exit(1);
}
console.log('OK — onboarding tour stays interactive while the background is disabled.');
