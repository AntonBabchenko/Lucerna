import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const loadInstanceIcon = vi.fn();
vi.mock('$lib/instances/instance-icon-cache', () => ({
  loadInstanceIcon: (id: string) => loadInstanceIcon(id),
  invalidateInstanceIcon: vi.fn(),
}));

import InstanceAvatar from '$lib/instances/InstanceAvatar.svelte';

const base = {
  id: 'i1',
  name: 'Skyblock',
  loader: 'fabric' as const,
  mrpack_source: null,
  has_icon: false,
};

describe('InstanceAvatar', () => {
  beforeEach(() => loadInstanceIcon.mockReset());

  it('renders the letter fallback when has_icon is false', () => {
    render(InstanceAvatar, { props: { instance: base } });
    expect(screen.getByText('S')).toBeTruthy();
    expect(loadInstanceIcon).not.toHaveBeenCalled();
  });

  it('renders an <img> when has_icon and the icon loads', async () => {
    loadInstanceIcon.mockResolvedValue('data:image/png;base64,AAAA');
    render(InstanceAvatar, { props: { instance: { ...base, has_icon: true } } });
    const img = await screen.findByRole('img');
    expect(img.getAttribute('src')).toBe('data:image/png;base64,AAAA');
  });
});
