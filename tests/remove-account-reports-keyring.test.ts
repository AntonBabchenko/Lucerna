/**
 * Remove account must report a sign-in token the keyring refused to delete.
 *
 * `remove_account` used to log the failure and return `()`; the account was
 * gone from disk and the token stayed in the OS keyring for good — ids are
 * random `ms-<uuid>`, so no later sign-in overwrites it (ACC-03 in the
 * 2026-09-21 Settings audit). The command now returns `RemovedAccount`, and
 * the one place that awaits it must turn `keyring_cleared: false` into a
 * warning the user can act on.
 *
 * `+page.svelte` is the whole app shell and is not renderable under vitest,
 * so this reads the handler's source instead — the same guard-by-source-scan
 * shape as `external-change-no-relist.test.ts`.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const PAGE = resolve('src/routes/+page.svelte');

/** The body of `confirmRemoveAccount`, from its declaration to the next top-level function. */
function removeHandlerBody(src: string): string {
  const start = src.indexOf('async function confirmRemoveAccount()');
  expect(start, 'confirmRemoveAccount is declared in +page.svelte').toBeGreaterThan(-1);
  const end = src.indexOf('\n  async function ', start + 1);
  return src.slice(start, end > start ? end : undefined);
}

describe('Remove account reports a token the keyring kept', () => {
  const body = removeHandlerBody(readFileSync(PAGE, 'utf8'));

  it('reads the keyring outcome the command now returns', () => {
    expect(body).toContain('keyring_cleared');
  });

  it('warns with the dedicated sentence and the keyring reason', () => {
    expect(body).toContain('pushWarning(');
    expect(body).toContain("'page.accounts.removeKeyringFailed'");
    expect(body).toContain('result.data.details');
  });

  it('still refreshes the account list — the account is gone either way', () => {
    expect(body).toContain('await refreshAccounts();');
  });
});
