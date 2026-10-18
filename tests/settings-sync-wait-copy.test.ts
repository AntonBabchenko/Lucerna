/**
 * The options row waits for «Minecraft to save its options once» when the instance's options.txt
 * was not written by its own version. Measured on 1.21.1 (v0.28 regression, F4): a launch to the
 * menu and a graceful close leave the file untouched — 1.21.x calls `Options.save` on a
 * fullscreen toggle/error and a resource-pack reset, not at startup or exit. Leaving the Options
 * screen saves on every supported version (`OptionsScreen.removed()` / `OptionsSubScreen.removed()`).
 * So the state must send the user there, not tell them to launch and close the game.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

const read = (locale: string) =>
  JSON.parse(readFileSync(resolve(`src/lib/i18n/locales/${locale}.json`), 'utf8')).instance.settingsSync.state
    .optionsGameSave as string;

describe('options waiting state copy', () => {
  it('EN sends the user to the Options screen', () => {
    const s = read('en');
    expect(s).toMatch(/Options/);
    expect(s).toMatch(/Done/);
    expect(s).not.toMatch(/launch the game and close it/);
  });

  it('RU sends the user to the Options screen', () => {
    const s = read('ru');
    expect(s).toMatch(/«Настройки»/);
    expect(s).toMatch(/«Готово»/);
    expect(s).not.toMatch(/запустите игру и закройте её/);
  });
});
