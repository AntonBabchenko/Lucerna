import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import { ICON_NAMES, ICONS, type IconName } from '$lib/ui/icons';
import Icon from '$lib/ui/icons/Icon.svelte';

describe('icon registry', () => {
  it('resolves a component for every registered name', () => {
    for (const name of ICON_NAMES) {
      expect(ICONS[name], `icon "${name}" must resolve`).toBeTruthy();
    }
  });

  it('exposes the names expected by the UI', () => {
    const expected: IconName[] = [
      'caret',
      'chevronDown',
      'chevronUp',
      'close',
      'warning',
      'success',
      'download',
      'update',
      'refresh',
      'plus',
      'minus',
      'list',
      'grid',
      'package',
      'externalLink',
      'moreVertical',
      'chevronLeft',
      'chevronRight',
      'puzzle',
      'arrowRight',
      'settings',
      'sliders',
      'stop',
      'userPlus',
      'folderOpen',
      'trash',
      'blocks',
      'scrollText',
      'arrowUp',
      'circleX',
      'shrink',
      'expand',
      'resourcePack',
      'shader',
      'chevronFirst',
      'chevronLast',
      'arrowLeft',
      'arrowUpRight',
      'power',
      'user',
      'switch',
      'lock',
      'archive',
      'restore',
    ];
    for (const n of expected) expect(ICON_NAMES).toContain(n);
  });

  it('gives the AI-translate action a glyph of its own', () => {
    expect(ICON_NAMES).toContain('aiTranslate');
    // Intent-based naming exists so one glyph never carries two unrelated
    // meanings. Sparkles was the skin editor's brightness-jitter tool before
    // the l10n AI action needed it, so `noise` moved rather than doubling up.
    expect(ICONS.aiTranslate).not.toBe(ICONS.noise);
  });
});

describe('Icon component', () => {
  it('renders an svg and is decorative (aria-hidden) by default', () => {
    const { container } = render(Icon, { props: { name: 'success' } });
    const svg = container.querySelector('svg');
    expect(svg).toBeTruthy();
    expect(svg?.getAttribute('aria-hidden')).toBe('true');
  });

  it('exposes an accessible label when one is given', () => {
    const { container } = render(Icon, { props: { name: 'warning', label: 'Warning' } });
    const svg = container.querySelector('svg');
    expect(svg?.getAttribute('aria-hidden')).toBeNull();
    expect(svg?.getAttribute('aria-label')).toBe('Warning');
    expect(svg?.getAttribute('role')).toBe('img');
  });

  it('forwards class for sizing/colour overrides', () => {
    const { container } = render(Icon, { props: { name: 'close', class: 'size-3 text-muted' } });
    expect(container.querySelector('svg')?.getAttribute('class')).toContain('size-3');
  });
});
