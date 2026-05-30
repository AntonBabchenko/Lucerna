import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import ImageGallery from '$lib/ui/ImageGallery.svelte';

const imgs = [
  { url: 'https://x/1.png', title: 'One' },
  { url: 'https://x/2.png', title: 'Two' },
];

describe('ImageGallery', () => {
  it('renders nothing when empty', () => {
    const { container } = render(ImageGallery, { props: { images: [] } });
    expect(container.querySelector('img')).toBeNull();
  });

  it('shows the first image and advances on Next', async () => {
    render(ImageGallery, { props: { images: imgs } });
    const img = screen.getByRole('img') as HTMLImageElement;
    expect(img.src).toContain('1.png');
    await fireEvent.click(screen.getByLabelText('Next image'));
    expect((screen.getByRole('img') as HTMLImageElement).src).toContain('2.png');
  });

  it('shows a position counter', () => {
    render(ImageGallery, { props: { images: imgs } });
    expect(screen.getByText('1 / 2')).toBeTruthy();
  });
});
