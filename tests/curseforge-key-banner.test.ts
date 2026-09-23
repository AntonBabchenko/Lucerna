// The CurseForge banners open Settings AT the key field — not at the top of
// Integrations — so the user lands where the banner sent them.
import { fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { beforeEach, describe, expect, it } from 'vitest';
import CurseForgeKeyBanner from '$lib/mods/CurseForgeKeyBanner.svelte';
import { settingsOpen, settingsSearchFocus } from '$lib/settings/state.svelte';

describe('CurseForgeKeyBanner', () => {
  beforeEach(() => {
    settingsOpen.value = null;
    settingsSearchFocus.value = null;
  });

  it('opens Settings at the CurseForge key field', async () => {
    render(CurseForgeKeyBanner);
    await fireEvent.click(screen.getByRole('button', { name: /open settings/i }));
    await tick();
    expect(settingsOpen.value).toEqual({ tab: 'integrations' });
    expect(settingsSearchFocus.value).toBe('integrations.curseforgeKey');
  });

  it('names the tab the key lives on', () => {
    // The tab has been "Integrations" since 2026-06-15; the button still said CurseForge.
    render(CurseForgeKeyBanner);
    expect(screen.getByRole('button', { name: 'Open Settings → Integrations' })).toBeTruthy();
  });
});
