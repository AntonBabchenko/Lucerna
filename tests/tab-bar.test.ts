import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import TabBar from '$lib/ui/TabBar.svelte';

const tabs = [
  { id: 'overview', label: 'Overview' },
  { id: 'versions', label: 'Versions' },
];

describe('TabBar', () => {
  it('marks the active tab selected', () => {
    render(TabBar, { props: { tabs, active: 'overview', onChange: () => {} } });
    expect(screen.getByRole('tab', { name: 'Overview' }).getAttribute('aria-selected')).toBe(
      'true',
    );
  });

  it('calls onChange when another tab is clicked', async () => {
    const onChange = vi.fn();
    render(TabBar, { props: { tabs, active: 'overview', onChange } });
    await fireEvent.click(screen.getByRole('tab', { name: 'Versions' }));
    expect(onChange).toHaveBeenCalledWith('versions');
  });

  it("applies a per-tab iconClass to that tab's icon", () => {
    render(TabBar, {
      props: {
        tabs: [{ id: 'a', label: 'Alpha', icon: 'shader', iconClass: 'icon-rainbow-hover' }],
        active: 'a',
        onChange: () => {},
      },
    });
    const svg = screen.getByRole('tab', { name: 'Alpha' }).querySelector('svg');
    expect(svg?.classList.contains('icon-rainbow-hover')).toBe(true);
  });

  it('wires aria-controls and a matching id on each tab when panelId is set', () => {
    render(TabBar, {
      props: { tabs, active: 'overview', onChange: () => {}, panelId: 'my-panel' },
    });
    const overviewTab = screen.getByRole('tab', { name: 'Overview' });
    const versionsTab = screen.getByRole('tab', { name: 'Versions' });
    expect(overviewTab.getAttribute('aria-controls')).toBe('my-panel');
    expect(overviewTab.getAttribute('id')).toBe('my-panel-tab-overview');
    expect(versionsTab.getAttribute('id')).toBe('my-panel-tab-versions');
  });

  it('omits aria-controls and id entirely when panelId is not provided (default — every existing consumer)', () => {
    render(TabBar, { props: { tabs, active: 'overview', onChange: () => {} } });
    const overviewTab = screen.getByRole('tab', { name: 'Overview' });
    expect(overviewTab.hasAttribute('aria-controls')).toBe(false);
    expect(overviewTab.hasAttribute('id')).toBe(false);
  });
});
