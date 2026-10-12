// MainTabs' fifth tab, Settings: where it sits, what it renders, and how a
// request from elsewhere (an Overview card, the sidebar) switches to it.

import { fireEvent, render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  clearInstanceSettingsNav,
  instanceSettingsNav,
} from '$lib/instances/settings/instance-settings-nav.svelte';
import MainTabs from '$lib/layout/MainTabs.svelte';
import { clientActiveTab } from '$lib/settings/state.svelte';

vi.mock('$lib/ipc/bindings', () => ({ commands: {}, events: {} }));
vi.mock('$lib/worlds/WorldsTab.svelte', () => ({
  default: function stubWorldsTab() {
    return {};
  },
}));
vi.mock('$lib/screenshots/ScreenshotsTab.svelte', () => ({
  default: function stubScreenshotsTab() {
    return {};
  },
}));

let overviewRenders = 0;
const overview = createRawSnippet(() => ({
  render: () => {
    overviewRenders++;
    return '<div data-testid="overview-body">overview</div>';
  },
}));
const settings = createRawSnippet(() => ({
  render: () => '<div data-testid="settings-body">settings</div>',
}));

function mount(over: Record<string, unknown> = {}) {
  return render(MainTabs, { props: { overview, settings, instanceId: 'inst-1', ...over } });
}

beforeEach(() => {
  overviewRenders = 0;
  clearInstanceSettingsNav();
});
afterEach(() => {
  clientActiveTab.value = null;
});

describe('MainTabs — the Settings tab', () => {
  it('is the last of five tabs and carries the tour anchor', () => {
    mount();
    const tabs = screen.getAllByRole('tab');
    expect(tabs.map((t) => t.textContent?.trim())).toEqual([
      'Overview',
      'Add-ons',
      'Worlds',
      'Screenshots',
      'Settings',
    ]);
    expect(tabs[4].getAttribute('data-tour')).toBe('tab-settings');
    // The accessible name keeps the visible word and says whose settings.
    expect(tabs[4].getAttribute('aria-label')).toBe('Instance settings');
  });

  it('renders the settings snippet when chosen, and mirrors it for the drop router', async () => {
    mount();
    await fireEvent.click(screen.getAllByRole('tab')[4]);
    expect(screen.getByTestId('settings-body')).toBeTruthy();
    expect(clientActiveTab.value).toBe('settings');
  });

  it('a request for the active instance switches to it', async () => {
    mount();
    instanceSettingsNav.value = { instanceId: null, field: 'memory' };
    await Promise.resolve();
    expect(await screen.findByTestId('settings-body')).toBeTruthy();
  });

  it('a request for another instance waits until that instance is active', async () => {
    const { rerender } = mount();
    instanceSettingsNav.value = { instanceId: 'inst-2', field: null };
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByTestId('settings-body')).toBeNull();
    await rerender({ overview, settings, instanceId: 'inst-2' });
    expect(await screen.findByTestId('settings-body')).toBeTruthy();
  });

  it('mounting with a request pending starts on Settings — Overview never renders', () => {
    instanceSettingsNav.value = { instanceId: 'inst-1', field: null };
    mount();
    expect(screen.getByTestId('settings-body')).toBeTruthy();
    expect(overviewRenders).toBe(0);
  });

  it('a click on another tab drops a request still waiting', async () => {
    mount();
    instanceSettingsNav.value = { instanceId: 'inst-2', field: null };
    await fireEvent.click(screen.getAllByRole('tab')[2]); // Worlds
    expect(instanceSettingsNav.value).toBeNull();
  });

  it('arrow keys reach it, and Home/End wrap over five tabs', async () => {
    mount();
    const tabs = screen.getAllByRole('tab');
    tabs[0].focus();
    await fireEvent.keyDown(tabs[0], { key: 'End' });
    expect(tabs[4].getAttribute('aria-selected')).toBe('true');
    await fireEvent.keyDown(tabs[4], { key: 'ArrowRight' });
    expect(tabs[0].getAttribute('aria-selected')).toBe('true');
  });
});
