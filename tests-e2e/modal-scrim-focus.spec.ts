import { expect, type Page, test } from '@playwright/test';
import { installMockIpc, makeInstance } from './helpers/mock-ipc';

// Real-input regression tests for the shared Modal's focus (2026-10-05). A press on a dialog's
// scrim released inside its panel kept the dialog open — it closes only on a press AND a release on
// the scrim — but the press dropped the focus to <body>, since the scrim is not focusable. With the
// focus trap listening on the panel, Shift+Tab then walked the page behind the open dialog. Any
// other loss of the focus did the same: a toast's button above the dialog that removes its toast, a
// focused control that is removed. Synthetic events (the unit tests) move no focus, so only a real
// browser shows either.

test.beforeEach(async ({ page }) => {
  await installMockIpc(page, {
    instances: [makeInstance({ id: 'inst-1', name: 'Vanilla' })],
    active_instance_id: 'inst-1',
  });
  await page.goto('/');
});

type IpcCall = { cmd: string; args: unknown };

/** Every command the page sent, in order (logged by the mock IPC). */
const ipcCalls = (page: Page): Promise<IpcCall[]> =>
  page.evaluate(() => (window as unknown as { __mockIpcCalls?: IpcCall[] }).__mockIpcCalls ?? []);

/** Press on the scrim's top-left corner, drag into the panel, release there. */
async function pressScrimReleaseInPanel(page: Page) {
  const box = await page.getByRole('dialog').boundingBox();
  if (!box) throw new Error('the dialog has no box');
  await page.mouse.move(4, 4);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2, box.y + 40, { steps: 6 });
  await page.mouse.up();
}

test('a press on the scrim released in the panel keeps the focus where it was', async ({
  page,
}) => {
  await page.getByRole('button', { name: 'Browse modpacks' }).click();
  const dialog = page.getByRole('dialog');
  const search = dialog.getByPlaceholder(/^Search modpacks/);
  await search.click();
  await expect(search).toBeFocused();

  await pressScrimReleaseInPanel(page);

  await expect(dialog).toBeVisible();
  await expect(search).toBeFocused();
  // And the keyboard stays in the dialog.
  await page.keyboard.press('Shift+Tab');
  await expect(dialog.locator(':focus')).toHaveCount(1);
});

test('Tab with the focus on nothing lands in the dialog, not the page behind it', async ({
  page,
}) => {
  await page.getByRole('button', { name: 'Browse modpacks' }).click();
  const dialog = page.getByRole('dialog');
  const first = dialog.getByRole('button', {
    name: 'Why does installing a modpack create a new instance?',
  });
  await expect(first).toBeFocused();
  const dropFocus = () => page.evaluate(() => (document.activeElement as HTMLElement).blur());

  // The first control loses the focus (a removed toast or control leaves the same <body>):
  // Shift+Tab from where it stood would leave the dialog. It lands on the dialog's last
  // control — the one from which Tab wraps to the first.
  await dropFocus();
  await page.keyboard.press('Shift+Tab');
  await expect(dialog.locator(':focus')).toHaveCount(1);
  await page.keyboard.press('Tab');
  await expect(first).toBeFocused();

  // The last control loses the focus: Tab from where it stood would leave the dialog too. It
  // comes back to the first.
  await page.keyboard.press('Shift+Tab');
  await dropFocus();
  await page.keyboard.press('Tab');
  await expect(first).toBeFocused();
});

test('a click on the scrim leaves the field first: the name being typed is saved', async ({
  page,
}) => {
  await page.locator('[data-tour="manage-btn"]').click();
  const dialog = page.getByRole('dialog');
  const name = dialog.locator('#detail-name');
  await name.fill('Vanilla Plus');
  await expect(name).toBeFocused();

  await page.mouse.click(4, 4); // press and release on the scrim

  await expect(dialog).toBeHidden();
  await expect
    .poll(
      async () => (await ipcCalls(page)).find((c) => c.cmd === 'set_instance_name')?.args ?? null,
    )
    .toEqual({ id: 'inst-1', name: 'Vanilla Plus' });
  // The save was still on its way when the dialog closed. The profile list is read again after
  // it all the same, so the sidebar shows the new name.
  await expect
    .poll(async () => {
      const calls = await ipcCalls(page);
      const saved = calls.findIndex((c) => c.cmd === 'set_instance_name');
      return calls.slice(saved + 1).some((c) => c.cmd === 'list_instances');
    })
    .toBe(true);
});
