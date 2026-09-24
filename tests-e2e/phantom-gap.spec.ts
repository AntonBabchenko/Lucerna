import { expect, type Locator, test } from '@playwright/test';
import { installMockIpc, makeInstance } from './helpers/mock-ipc';

// An empty in-flow element in a flex/grid container still takes a gap slot:
// Settings → Appearance showed a double gap between "Add account" and "Manage"
// (an idle status line). The source scan in tests/no-idle-live-region-box.test.ts
// bans the known shapes; this measures the real layout, which also catches a
// shape the scan cannot see (an empty element handed to a wrapping component).

const offlineAccount = {
  id: 'of-1',
  name: 'Steve',
  uuid: '00000000-0000-0000-0000-000000000001',
  expires_at: null,
};

async function openSettings(page: import('@playwright/test').Page) {
  await installMockIpc(page, {
    accounts: [offlineAccount],
    active_account_id: 'of-1',
    instances: [makeInstance({ id: 'inst-1', name: 'Default' })],
    active_instance_id: 'inst-1',
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toBeVisible();
  return dialog;
}

/** Every rendered, in-flow, zero-size element whose flex/grid parent spaces its
 *  children with a gap on that axis — each one costs its container a gap. */
function phantomGapItems(panel: Locator): Promise<string[]> {
  return panel.evaluate((root) => {
    const out: string[] = [];
    for (const el of root.querySelectorAll<HTMLElement>('*')) {
      const parent = el.parentElement;
      if (!parent || parent.getClientRects().length === 0) continue;
      if (el.closest('details:not([open])') && el.tagName !== 'SUMMARY') continue;
      const ps = getComputedStyle(parent);
      if (!/(flex|grid)$/.test(ps.display)) continue;
      const s = getComputedStyle(el);
      if (s.position === 'absolute' || s.position === 'fixed') continue;
      if (s.display === 'none' || s.display === 'contents' || el.tagName === 'LEGEND') continue;
      const column = ps.display.endsWith('grid') || ps.flexDirection.startsWith('column');
      const gap = Number.parseFloat(column ? ps.rowGap : ps.columnGap) || 0;
      const r = el.getBoundingClientRect();
      if (gap > 0 && (column ? r.height : r.width) === 0) {
        const id = el.getAttribute('data-testid') ?? el.getAttribute('role') ?? '';
        out.push(`${el.tagName.toLowerCase()}[${id}] in ${parent.tagName.toLowerCase()}.${parent.className}`);
      }
    }
    return out;
  });
}

const TABS = ['Appearance', 'Game', 'Integrations', 'Privacy & network', 'Storage', 'Updates', 'Help'];

test('no idle element takes a gap slot on any Settings page', async ({ page }) => {
  const dialog = await openSettings(page);
  const panel = dialog.getByTestId('settings-panel');
  for (const name of TABS) {
    await dialog.getByRole('tab', { name, exact: true }).click();
    await expect(panel.locator('h3').first()).toBeVisible();
    expect(await phantomGapItems(panel), name).toEqual([]);
  }
});

test('the sidebar-button rows are spaced by the list gap only', async ({ page }) => {
  const dialog = await openSettings(page);
  const row = (id: string) => dialog.getByTestId(`sidebar-button-toggle-${id}`).locator('xpath=..');
  const above = await row('account_actions').boundingBox();
  const below = await row('manage').boundingBox();
  const gap = await row('manage').evaluate((el) =>
    Number.parseFloat(getComputedStyle(el.parentElement as HTMLElement).rowGap),
  );
  expect(above && below).toBeTruthy();
  if (!above || !below) return;
  expect(Math.round(below.y - (above.y + above.height))).toBe(Math.round(gap));
});
