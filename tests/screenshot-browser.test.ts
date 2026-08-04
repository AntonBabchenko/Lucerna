import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { Screenshot } from '$lib/ipc/bindings';

vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    screenshotThumbnail: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: 'data:image/jpeg;base64,x' }),
    screenshotPreview: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: 'data:image/jpeg;base64,x' }),
  },
}));

import ScreenshotBrowser from '$lib/screenshots/ScreenshotBrowser.svelte';
import { screenshotGranularity, screenshotSortDir } from '$lib/settings/state.svelte';

// One shot per day going backwards, so day grouping produces many groups.
function makeShots(count: number): Screenshot[] {
  const now = new Date();
  return Array.from({ length: count }, (_, i) => ({
    instance_id: 'inst-1',
    instance_name: 'Instance One',
    file_name: `shot-${i}.png`,
    size_bytes: 1024,
    modified_unix_ms: new Date(now.getFullYear(), now.getMonth(), now.getDate() - i).getTime(),
  }));
}

function cardCount(): number {
  return screen.queryAllByTestId('screenshot-card').length;
}

function resetPreferences() {
  screenshotGranularity.value = 'day';
  screenshotSortDir.value = 'newest';
}

describe('ScreenshotBrowser paging', () => {
  it('renders at most one page on first render', () => {
    resetPreferences();
    render(ScreenshotBrowser, { props: { shots: makeShots(130), resetKey: 'inst-1' } });
    expect(cardCount()).toBe(120);
  });

  it('reveals the rest when Show more is clicked', async () => {
    resetPreferences();
    render(ScreenshotBrowser, { props: { shots: makeShots(130), resetKey: 'inst-1' } });
    await fireEvent.click(screen.getByTestId('shots-show-more'));
    expect(cardCount()).toBe(130);
  });

  it('reports how many of the total are shown', () => {
    resetPreferences();
    render(ScreenshotBrowser, { props: { shots: makeShots(130), resetKey: 'inst-1' } });
    expect(screen.getByTestId('shots-count').textContent).toContain('120');
    expect(screen.getByTestId('shots-count').textContent).toContain('130');
  });

  it('drops the button and reports the total once everything is shown', () => {
    resetPreferences();
    render(ScreenshotBrowser, { props: { shots: makeShots(5), resetKey: 'inst-1' } });
    expect(screen.queryByTestId('shots-show-more')).toBeNull();
    expect(screen.getByTestId('shots-count').textContent).toContain('5');
  });
});

describe('ScreenshotBrowser reset rules', () => {
  it('resets to one page when the collection changes', async () => {
    resetPreferences();
    const { rerender } = render(ScreenshotBrowser, {
      props: { shots: makeShots(130), resetKey: 'inst-1' },
    });
    await fireEvent.click(screen.getByTestId('shots-show-more'));
    expect(cardCount()).toBe(130);

    await rerender({ shots: makeShots(130), resetKey: 'inst-2' });
    expect(cardCount()).toBe(120);
  });

  it('keeps the revealed pages when granularity changes', async () => {
    resetPreferences();
    render(ScreenshotBrowser, { props: { shots: makeShots(130), resetKey: 'inst-1' } });
    await fireEvent.click(screen.getByTestId('shots-show-more'));
    expect(cardCount()).toBe(130);

    await fireEvent.click(screen.getByTestId('shots-granularity-month'));
    expect(cardCount()).toBe(130);
  });

  it('resets to one page when the sort direction changes', async () => {
    resetPreferences();
    render(ScreenshotBrowser, { props: { shots: makeShots(130), resetKey: 'inst-1' } });
    await fireEvent.click(screen.getByTestId('shots-show-more'));
    expect(cardCount()).toBe(130);

    await fireEvent.click(screen.getByTestId('shots-sort-oldest'));
    expect(cardCount()).toBe(120);
  });
});
