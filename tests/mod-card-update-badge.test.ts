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
// update badge and was cut mid-glyph. Plan §5d M1 (n01j): shrinking in proportion still cut the
// name («Fabric A…») while the version kept a 2.7 px sliver. The version now takes only the room
// left once the name is whole: from a few characters up it shows, ending in «…»; with less it
// wraps onto a line the row clips, and the name is cut only once it alone does not fit. Only a
// browser lays this out: these pin the structure the re-render measures.
describe('ModCard in a narrow row', () => {
  const withSummary = { ...base, summary: { ...summary, summary: 'Does things' } };

  it('gives the version the room the whole name leaves — a few characters or none', () => {
    render(ModCard, { props: withSummary });
    const version = screen.getByTestId('mod-version');
    expect(version.classList).toContain('truncate');
    expect(version.classList).toContain('min-w-0');
    // Its node (with its pin) starts from a few characters and grows to its whole width before the
    // description gets any room; it never shrinks in proportion with the name.
    const node = version.parentElement as HTMLElement;
    expect(node.classList).toContain('basis-[4ch]');
    expect(node.classList).toContain('grow-[1000]');
    expect(node.classList).toContain('max-w-max');
    expect(node.classList).toContain('min-w-0');
    expect(node.className).not.toMatch(/\bshrink-\[/);
    // Less room than that: the node wraps onto a second line, which the one-line row clips.
    const line = node.parentElement as HTMLElement;
    expect(line.tagName).toBe('BUTTON');
    for (const c of ['flex-wrap', 'h-5', 'overflow-hidden']) expect(line.classList).toContain(c);
    // The name keeps its width until it alone does not fit, then ends in «…».
    const name = screen.getByText('Alpha');
    expect(name.parentElement).toBe(line);
    expect(name.classList).toContain('truncate');
    expect(name.classList).toContain('min-w-0');
    expect(name.className).not.toMatch(/\b(grow|basis-|flex-1)/);
    // The description comes after the version and takes only what the version leaves.
    const description = screen.getByText('Does things');
    expect(description.parentElement).toBe(line);
    expect(node.compareDocumentPosition(description) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(description.classList).toContain('flex-1');
  });

  it('keeps room for the pin beside a held version', () => {
    render(ModCard, { props: { ...base, updateState: null, held: true } });
    const node = screen.getByTestId('mod-version').parentElement as HTMLElement;
    expect(node.contains(screen.getByTestId('mod-held-pin'))).toBe(true);
    expect(node.classList).toContain('basis-[calc(4ch+1rem)]');
    expect(node.classList).not.toContain('basis-[4ch]');
  });

  it('leaves a catalogue row as it was: nothing there wraps away', () => {
    render(ModCard, { props: { ...withSummary, installed: null, updateState: null } });
    const line = screen.getByText('Alpha').parentElement as HTMLElement;
    expect(line.classList).not.toContain('flex-wrap');
    expect(line.classList).not.toContain('overflow-hidden');
  });

  // With the version wrapped out of the line, pointing at the name shows what it no longer does.
  it('lets the name carry the version while the version has stepped aside', () => {
    render(ModCard, { props: { ...base, updateState: null } });
    const node = screen.getByTestId('mod-version').parentElement as HTMLElement;
    const line = node.parentElement as HTMLElement;
    const name = screen.getByText('Alpha');
    line.getBoundingClientRect = () => ({ top: 100, bottom: 120, height: 20 }) as DOMRect;
    // On the line: the name fits and says nothing more.
    node.getBoundingClientRect = () => ({ top: 102, bottom: 118, height: 16 }) as DOMRect;
    revealTooltip(name);
    expect(tooltipState.visible).toBe(false);
    // Wrapped onto the clipped second line.
    node.getBoundingClientRect = () => ({ top: 128, bottom: 144, height: 16 }) as DOMRect;
    revealTooltip(name);
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('Alpha · v1.0 · a.jar');
    hideTooltip();
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
