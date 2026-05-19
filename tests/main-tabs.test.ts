import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import MainTabs from '$lib/layout/MainTabs.svelte';

// Task 14 made the Mod browser tab mount the real ModBrowseView, which
// fires modsGetCurseforgeKeyStatus + modsSearch on mount. Stub both so
// the unrelated MainTabs assertions below don't trip on tauri-api
// errors from those background calls.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    modsGetCurseforgeKeyStatus: vi.fn().mockResolvedValue({ status: 'ok', data: 'set' }),
    modsSearch: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { hits: [], total: 0, offset: 0, page_size: 20 } }),
  },
}));

describe('MainTabs', () => {
  it('renders the three tab labels', () => {
    const { getByText } = render(MainTabs, { props: {} });
    expect(getByText('Overview')).toBeTruthy();
    expect(getByText('Mod browser')).toBeTruthy();
    expect(getByText('Modpacks')).toBeTruthy();
  });

  it('starts on Overview tab', () => {
    const { getByText } = render(MainTabs, { props: {} });
    const overview = getByText('Overview').closest('button');
    expect(overview?.getAttribute('aria-selected')).toBe('true');
  });

  it('switches active tab on click', async () => {
    const { getByText } = render(MainTabs, { props: {} });
    await fireEvent.click(getByText('Mod browser'));
    const browser = getByText('Mod browser').closest('button');
    expect(browser?.getAttribute('aria-selected')).toBe('true');
  });

  it('renders ModBrowserTab when Mod browser tab is active', async () => {
    const { getByText, getByLabelText } = render(MainTabs, { props: {} });
    await fireEvent.click(getByText('Mod browser'));
    // ModBrowserTab renders the Browse / Installed sub-tabs and the
    // Source picker — assert on those rather than placeholder text.
    expect(getByText('Browse')).toBeTruthy();
    expect(getByText('Installed')).toBeTruthy();
    expect(getByLabelText('Mod source')).toBeTruthy();
  });

  it('renders placeholder content for Modpacks tab', async () => {
    const { getByText } = render(MainTabs, { props: {} });
    await fireEvent.click(getByText('Modpacks'));
    expect(getByText(/Coming in v0\.5\.0 modpack import slice/i)).toBeTruthy();
  });
});
