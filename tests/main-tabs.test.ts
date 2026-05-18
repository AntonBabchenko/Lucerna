import { fireEvent, render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import MainTabs from '$lib/layout/MainTabs.svelte';

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

  it('renders placeholder content for Mod browser tab', async () => {
    const { getByText } = render(MainTabs, { props: {} });
    await fireEvent.click(getByText('Mod browser'));
    expect(getByText(/Coming in v0\.5\.0 mod browser slice/i)).toBeTruthy();
  });

  it('renders placeholder content for Modpacks tab', async () => {
    const { getByText } = render(MainTabs, { props: {} });
    await fireEvent.click(getByText('Modpacks'));
    expect(getByText(/Coming in v0\.5\.0 modpack import slice/i)).toBeTruthy();
  });
});
