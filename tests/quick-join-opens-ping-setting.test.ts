/**
 * QuickJoin's "Enable in Settings" must open Settings AT the server-status
 * permission (GAME-15), not at the top of Game. `+page.svelte` is the whole
 * app shell and is not renderable under vitest, so this reads the handler's
 * source — the same guard-by-source-scan shape as
 * `external-change-no-relist.test.ts`.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const PAGE = resolve('src/routes/+page.svelte');

function handlerBody(src: string): string {
  const start = src.indexOf('onOpenPingSetting={');
  expect(start, 'QuickJoin gets an onOpenPingSetting handler').toBeGreaterThan(-1);
  const end = src.indexOf('}}', start);
  return src.slice(start, end > start ? end : undefined);
}

describe('QuickJoin → Settings', () => {
  it('opens Settings at the server-status permission, through openSettingsAt', () => {
    const body = handlerBody(readFileSync(PAGE, 'utf8'));
    expect(body).toContain("openSettingsAt('game.serverPing')");
    expect(body).not.toContain("settingsOpen.value = { tab: 'game' }");
  });
});
