// Settings → Integrations gains the AI translation section: the consent gate
// for the whole pre-fill feature, plus the provider / model / credential it
// needs. Five things are pinned here, and each one is a promise the feature
// makes to the user before they spend anything:
//
//   1. the permission starts OFF and persists without clobbering a sibling
//      panel's field (the panel owns four GeneralSettings fields, no more);
//   2. the credential controls follow the provider — a local model needs a
//      port and no key, a hosted one the reverse;
//   3. a stored key is never read back into the UI, at the DOM *and* at the
//      IPC surface, so the second can't quietly outgrow the first;
//   4. nothing below the provider picker survives a switch, so one provider's
//      key status is never shown (or its pasted key filed) under another;
//   5. what the feature does NOT translate is on screen before the user
//      turns anything on, not discovered afterwards.
//
// `vi.mock` is hoisted above the `const` declarations below, so every entry in
// the factory has to be an arrow that reaches its binding lazily rather than
// closing over an undefined value (see tests/server-ping-setting.test.ts).

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
// Static import on purpose: a dynamic `await import()` of a .svelte module
// inside a test body never resolves under this vitest setup. `vi.mock` is
// hoisted above imports anyway, so the panel still sees the mocked bindings.
import AiTranslationSection from '$lib/settings/AiTranslationSection.svelte';

const appSettingsGet = vi.fn();
const appSettingsSetGeneral = vi.fn();
const l10nPrefillKeyStatus = vi.fn();
const l10nPrefillSetKey = vi.fn();
const l10nPrefillTestKey = vi.fn();

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    appSettingsGet: () => appSettingsGet(),
    appSettingsSetGeneral: (g: unknown) => appSettingsSetGeneral(g),
    l10nPrefillKeyStatus: (p: unknown) => l10nPrefillKeyStatus(p),
    l10nPrefillSetKey: (p: unknown, k: unknown) => l10nPrefillSetKey(p, k),
    l10nPrefillTestKey: () => l10nPrefillTestKey(),
  },
}));

function general(over: Record<string, unknown> = {}) {
  return {
    hide_to_tray_during_game: false,
    theme: 'system',
    check_updates_on_startup: true,
    gpu_preference: 'auto',
    allow_ai_translation: false,
    ai_provider: 'anthropic',
    ai_model: '',
    ai_local_port: 11434,
    ...over,
  };
}

describe('Settings → Integrations: AI translation', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    locale.set('en');
    appSettingsGet.mockResolvedValue({ status: 'ok', data: { general: general() } });
    appSettingsSetGeneral.mockResolvedValue({ status: 'ok', data: null });
    l10nPrefillKeyStatus.mockResolvedValue({ status: 'ok', data: false });
    l10nPrefillSetKey.mockResolvedValue({ status: 'ok', data: null });
    l10nPrefillTestKey.mockResolvedValue({ status: 'ok', data: null });
  });

  it('starts off and persists the consent toggle without clobbering siblings', async () => {
    // `language` and `compact_mode` belong to other panels. The read-modify-write
    // must carry them through untouched.
    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { general: general({ language: 'ru', compact_mode: true }) },
    });
    render(AiTranslationSection);

    const toggle = await waitFor(
      () => screen.getByTestId('ai-translation-toggle') as HTMLInputElement,
    );
    expect(toggle.checked).toBe(false);

    await fireEvent.click(toggle);
    await waitFor(() =>
      expect(appSettingsSetGeneral).toHaveBeenCalledWith(
        expect.objectContaining({
          allow_ai_translation: true,
          language: 'ru',
          compact_mode: true,
        }),
      ),
    );
  });

  it('hides the API key field for the local provider', async () => {
    render(AiTranslationSection);
    // Default provider is a hosted one, so the key field is there to begin with.
    await waitFor(() => expect(screen.getByTestId('ai-key-input')).toBeTruthy());

    // Switch providers the way a user does — through the shared Select, which
    // commits on mousedown rather than click.
    await fireEvent.click(screen.getByRole('combobox'));
    await fireEvent.mouseDown(screen.getByRole('option', { name: /local/i }));

    await waitFor(() => expect(screen.queryByTestId('ai-key-input')).toBeNull());
    await waitFor(() =>
      expect(appSettingsSetGeneral).toHaveBeenCalledWith(
        expect.objectContaining({ ai_provider: 'local' }),
      ),
    );
  });

  it('shows the port field only for the local provider', async () => {
    const hosted = render(AiTranslationSection);
    await waitFor(() => screen.getByTestId('ai-translation-toggle'));
    expect(screen.queryByTestId('ai-local-port-input')).toBeNull();
    hosted.unmount();

    appSettingsGet.mockResolvedValue({
      status: 'ok',
      data: { general: general({ ai_provider: 'local', ai_local_port: 1234 }) },
    });
    render(AiTranslationSection);

    const port = await waitFor(() => screen.getByTestId('ai-local-port-input') as HTMLInputElement);
    expect(port.value).toBe('1234');
    // A small local model rejects more strings than a hosted one, and the
    // rejected ones stay English. Say it where the user picks the model, not
    // after they have waited for a run.
    expect(screen.getByTestId('ai-local-note').textContent).toMatch(/English/i);
  });

  it('never reads a stored key back into the UI', async () => {
    l10nPrefillKeyStatus.mockResolvedValue({ status: 'ok', data: true });
    render(AiTranslationSection);

    const input = await waitFor(() => screen.getByTestId('ai-key-input') as HTMLInputElement);
    // `toMatch(/stored/i)` would also accept "Not stored" — assert the exact
    // label so the affirmative branch is really the one on screen.
    await waitFor(() =>
      expect(screen.getByTestId('ai-key-status').textContent?.trim()).toBe('Stored'),
    );
    // The UI knows a key exists and still shows nothing.
    expect(input.value).toBe('');
    expect(input.getAttribute('type')).toBe('password');
    expect(l10nPrefillKeyStatus).toHaveBeenCalledWith('anthropic');

    // The DOM assertion above is only worth as much as the IPC surface behind
    // it: if some later command handed a secret back, an empty input would be
    // cosmetic. Every prefill key command must answer with a boolean or
    // nothing at all.
    const bindings = readFileSync(join('src', 'lib', 'ipc', 'bindings.ts'), 'utf8');
    const keyCommands = [
      ...bindings.matchAll(/\b(l10nPrefill\w*Key\w*): \([^)]*\) => typedError<([^,]+),/g),
    ];
    expect(keyCommands.length).toBeGreaterThan(0);
    for (const [, name, returns] of keyCommands) {
      expect(`${name} -> ${returns}`).toMatch(/-> (boolean|null)$/);
    }
  });

  it('does not carry one provider’s key status over to the next', async () => {
    // "A key is stored" is a fact about ONE provider. While the answer for the
    // newly picked provider is still in flight, leaving the previous one's
    // badge on screen tells the user a credential exists where none does —
    // and the Test-connection button next to it would then fail for a reason
    // the UI just denied.
    let resolveSecond: ((v: unknown) => void) | undefined;
    l10nPrefillKeyStatus.mockResolvedValueOnce({ status: 'ok', data: true }).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSecond = resolve;
        }),
    );
    render(AiTranslationSection);
    await waitFor(() =>
      expect(screen.getByTestId('ai-key-status').textContent?.trim()).toBe('Stored'),
    );

    await fireEvent.click(screen.getByRole('combobox'));
    await fireEvent.mouseDown(screen.getByRole('option', { name: /gemini/i }));

    await waitFor(() =>
      expect(screen.getByTestId('ai-key-status').textContent?.trim()).toBe('Checking…'),
    );
    expect(l10nPrefillKeyStatus).toHaveBeenLastCalledWith('gemini');
    // The second read really is still pending — otherwise "Checking…" would be
    // proving nothing.
    expect(resolveSecond).toBeTypeOf('function');
  });

  it('says what is out of scope before the user starts', async () => {
    render(AiTranslationSection);
    const toggle = await waitFor(
      () => screen.getByTestId('ai-translation-toggle') as HTMLInputElement,
    );
    // "Before the user starts" is the load-bearing half of this test: the
    // caveat has to be readable while the permission is still off.
    expect(toggle.checked).toBe(false);

    const scope = screen.getByTestId('ai-translation-scope').textContent ?? '';
    expect(scope).toMatch(/Patchouli/i);
    expect(scope).toMatch(/FTB Quests/i);
    expect(scope).toMatch(/config/i);
  });
});
