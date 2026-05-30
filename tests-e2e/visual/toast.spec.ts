import { expect, test } from '@playwright/test';
import { installMockIpc } from '../helpers/mock-ipc';
import { setTheme } from '../helpers/theme';

test.skip(process.platform !== 'linux', 'Visual tests pinned to Linux for cross-OS determinism');

const TOAST_HOST = '[data-testid="toast-host"]';

// The offline account and instance shapes mirror mock-ipc.ts defaults.
const offlineAccount = {
  id: 'of-1',
  name: 'Steve',
  uuid: '00000000-0000-0000-0000-000000000001',
  expires_at: null,
};

const baseInstance = {
  id: 'inst-1',
  name: 'Default',
  mc_version: '1.20.4',
  loader: 'vanilla' as const,
  loader_version: null,
  max_heap_mb: 2048,
  extra_jvm_args: '',
  created_unix_ms: null,
  ready: true,
  mrpack_name: null,
  mrpack_version: null,
  mrpack_project_id: null,
  mrpack_source: null,
  mrpack_summary: null,
  mrpack_version_id: null,
};

test.describe('Toast visual', () => {
  for (const theme of ['light', 'dark'] as const) {
    test(`success + warning toast row — ${theme}`, async ({ page }) => {
      await installMockIpc(page, {
        accounts: [offlineAccount],
        active_account_id: 'of-1',
        instances: [baseInstance],
        active_instance_id: 'inst-1',
        theme,
      });
      await page.goto('/');
      await setTheme(page, theme);

      // Inject toasts via Vite's source-module URL convention.
      // Note: on Windows these tests are skipped; Linux CI will surface
      // any Vite module-resolution issues on first run.
      await page.evaluate(async () => {
        // `as any` cast required here: window has no typed .location on
        // the evaluate context, and the dynamic import path is runtime-only.
        const mod = await import(
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          '/src/lib/toasts/toasts.svelte.ts' as any
        );
        (mod as { pushSuccess: (s: string) => void }).pushSuccess('Test success toast');
        (mod as { pushWarning: (s: string, lines?: string[]) => void }).pushWarning(
          'Test warning toast',
        );
      });

      await page.waitForSelector(`${TOAST_HOST} [data-testid="toast-success"]`);
      await page.waitForSelector(`${TOAST_HOST} [data-testid="toast-warning"]`);

      await expect(page.locator(TOAST_HOST)).toHaveScreenshot(`toast-row-${theme}.png`);
    });
  }
});
