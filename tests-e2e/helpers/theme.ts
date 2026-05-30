// Playwright helper: set the launcher's theme in a running page.
//
// The launcher uses:
//   - html.dark class (Tailwind darkMode: 'class' strategy) to apply dark tokens.
//   - localStorage key 'theme' (STORAGE_KEY in src/lib/theme/state.svelte.ts)
//     for persistence across navigations and the FOUC-prevention inline script
//     in src/app.html.
//
// setTheme() applies both so that:
//   1. The current render reflects the correct colour scheme immediately.
//   2. Any subsequent navigations / reactive reads see the persisted value.

import type { Page } from '@playwright/test';

export type Theme = 'light' | 'dark';

/**
 * Apply `theme` to the page's <html> element and persist it to localStorage.
 * Call after page.goto() so the DOM and storage are accessible.
 *
 * For FOUC-free dark-mode tests in visual snapshots, prefer setting the theme
 * via installMockIpc's `state.theme` field (which fires before any scripts
 * load) combined with calling setTheme() after navigation to guarantee the
 * class is present before a screenshot is taken.
 */
export async function setTheme(page: Page, theme: Theme): Promise<void> {
  await page.evaluate((t) => {
    // Mirror what src/lib/theme/state.svelte.ts does in applyClass() and
    // what the FOUC-prevention inline script in src/app.html does:
    document.documentElement.classList.toggle('dark', t === 'dark');
    try {
      localStorage.setItem('theme', t);
    } catch (_) {
      // Private-mode browsers may throw — degrade silently (same as app).
    }
  }, theme);
}
