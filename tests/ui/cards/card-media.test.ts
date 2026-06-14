import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import CardMedia from '$lib/ui/cards/CardMedia.svelte';

describe('CardMedia', () => {
  it('renders an <img> when iconUrl is given', () => {
    const { container } = render(CardMedia, { props: { iconUrl: 'https://x/i.png' } });
    expect(container.querySelector('img')?.getAttribute('src')).toBe('https://x/i.png');
  });

  it('renders a placeholder icon (no img) when iconUrl is null', () => {
    const { container } = render(CardMedia, { props: { iconUrl: null, placeholder: 'package' } });
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('svg')).toBeTruthy();
  });
});
