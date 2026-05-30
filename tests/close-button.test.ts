import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import CloseButton from '$lib/ui/CloseButton.svelte';

describe('CloseButton', () => {
  it('renders a × button with the default aria-label', () => {
    render(CloseButton);
    const btn = screen.getByRole('button', { name: 'Close' });
    expect(btn.tagName).toBe('BUTTON');
    expect(btn.textContent?.trim()).toBe('×');
    expect(btn.getAttribute('type')).toBe('button');
  });

  it('uses a custom aria-label when provided', () => {
    render(CloseButton, { props: { ariaLabel: 'Close error banner' } });
    const btn = screen.getByRole('button', { name: 'Close error banner' });
    expect(btn).toBeTruthy();
  });

  it('invokes onClick when clicked', async () => {
    const onClick = vi.fn();
    render(CloseButton, { props: { onClick } });
    await fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('merges extra class names with .btn-icon', () => {
    render(CloseButton, { props: { class: 'my-custom-class' } });
    const btn = screen.getByRole('button', { name: 'Close' });
    expect(btn.className).toContain('btn-icon');
    expect(btn.className).toContain('my-custom-class');
  });
});
