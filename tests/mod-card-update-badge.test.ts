import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import ModCard from '$lib/mods/ModCard.svelte';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import { revealTooltip } from './test-utils/reveal-tooltip';

const summary = {
  source: 'modrinth' as const,
  project_id: 'p',
  slug: 's',
  name: 'Alpha',
  summary: '',
  icon_url: null,
  downloads: 1,
  author: 'x',
  updated_at: null,
};
const installed = {
  filename: 'a.jar',
  sha1: 'a',
  source: 'modrinth' as const,
  project_id: 'p',
  version_id: 'v',
  name: 'Alpha',
  version_number: '1.0',
  installed_at: '2026-01-01T00:00:00Z',
  enabled: true,
  enrich_attempted: false,
};
const base = {
  summary,
  installed,
  layout: 'list' as const,
  onInstall() {},
  onOpenDetail() {},
  onToggle() {},
  onUninstall() {},
  updateState: { kind: 'update_available', target: { version_number: '2.0' } } as never,
};
beforeAll(() => locale.set('en'));

describe('ModCard update badge', () => {
  it('opens the changelog when the caller offers one', async () => {
    const onShowChangelog = vi.fn();
    render(ModCard, { props: { ...base, onShowChangelog } });
    const btn = screen.getByRole('button', { name: /v1\.0 → v2\.0, changelog/i });
    expect(btn.contains(screen.getByTestId('mod-update-badge'))).toBe(true);
    await fireEvent.click(btn);
    expect(onShowChangelog).toHaveBeenCalledOnce();
  });

  it('stays a static badge without one', () => {
    render(ModCard, { props: base });
    expect(screen.getByTestId('mod-update-badge').closest('button')).toBeNull();
  });

  // Plan §5b V1: «vmc1.21.1-0.13.1» in the row and the badge — the «v» goes only before a
  // version that starts with a digit.
  it('glues no «v» to a version that starts with a letter — row and badge alike', () => {
    render(ModCard, {
      props: {
        ...base,
        installed: { ...installed, version_number: 'mc1.21.1-0.13.1' },
        updateState: {
          kind: 'update_available',
          target: { version_number: 'mc1.21.1-0.13.2' },
        } as never,
        onShowChangelog: vi.fn(),
      },
    });
    expect(screen.getByTestId('mod-version').textContent).toBe('mc1.21.1-0.13.1');
    expect(screen.getByTestId('mod-update-badge').textContent).not.toContain('vmc');
    expect(
      screen.getByRole('button', { name: 'mc1.21.1-0.13.1 → mc1.21.1-0.13.2, changelog' }),
    ).toBeTruthy();
  });

  it('a held mod carries a pin that says updates are not offered', () => {
    render(ModCard, { props: { ...base, updateState: null, held: true } });
    expect(screen.getByRole('img', { name: 'Updates for this mod are not offered' })).toBeTruthy();
  });

  it('a row whose project details could not load carries the pin too', () => {
    render(ModCard, { props: { ...base, summary: null, updateState: null, held: true } });
    expect(screen.getByTestId('mod-held-pin')).toBeTruthy();
  });
});

// Plan §5c V3 (screenshot n01e): at 820 px «Fabric API v0.10|» — the version ran on under the
// update badge and was cut mid-glyph. It gives way first and ends in «…»; the name gives way only
// once the version is gone, and ends in «…» too. Only a browser lays this out: these pin the
// structure the re-render measures.
describe('ModCard in a narrow row', () => {
  it('lets the version give way first, then the name, each ending in «…»', () => {
    render(ModCard, { props: base });
    const version = screen.getByTestId('mod-version');
    expect(version.classList).toContain('truncate');
    expect(version.classList).toContain('min-w-0');
    // The version's own node (with its pin) shrinks, and far before the name does.
    const node = version.parentElement as HTMLElement;
    expect(node.classList).toContain('min-w-0');
    expect(node.classList).toContain('shrink-[1000]');
    expect(node.classList).not.toContain('flex-shrink-0');
    const name = screen.getByText('Alpha');
    expect(name.classList).toContain('truncate');
    expect(name.classList).toContain('min-w-0');
    expect(name.classList).not.toContain('flex-shrink-0');
  });

  it('keeps the whole version in its tooltip while it is cut short', () => {
    render(ModCard, { props: { ...base, updateState: null } });
    const version = screen.getByTestId('mod-version');
    Object.defineProperty(version, 'scrollWidth', { value: 80, configurable: true });
    Object.defineProperty(version, 'clientWidth', { value: 30, configurable: true });
    revealTooltip(version);
    expect(tooltipState.text).toBe('v1.0 · a.jar');
    hideTooltip();
  });
});
