import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';

import ModCard from '$lib/mods/ModCard.svelte';

const summary = {
  source: 'modrinth',
  project_id: 'abc',
  slug: 'sodium',
  name: 'Sodium',
  summary: 'Fast rendering',
  icon_url: null,
  downloads: 0,
  author: 'caffeine',
  updated_at: null,
} as never;

describe('ModCard install busy state', () => {
  it('shows a spinner and disables Install when installing', () => {
    const { getByRole } = render(ModCard, {
      props: {
        summary,
        installed: null,
        installing: true,
        onInstall: () => {},
        onOpenDetail: () => {},
        onToggle: () => {},
        onUninstall: () => {},
      } as never,
    });
    const btn = getByRole('button', { name: /install/i });
    expect(btn.hasAttribute('disabled')).toBe(true);
    expect(btn.querySelector('[role="status"]')).not.toBeNull();
  });

  it('Install is enabled with no spinner when not installing', () => {
    const { getByRole } = render(ModCard, {
      props: {
        summary,
        installed: null,
        installing: false,
        onInstall: () => {},
        onOpenDetail: () => {},
        onToggle: () => {},
        onUninstall: () => {},
      } as never,
    });
    const btn = getByRole('button', { name: /install/i });
    expect(btn.hasAttribute('disabled')).toBe(false);
    expect(btn.querySelector('[role="status"]')).toBeNull();
  });
});
