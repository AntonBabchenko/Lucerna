import { fireEvent, render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { describe, expect, it, vi } from 'vitest';
import Banner from '$lib/ui/Banner.svelte';

const body = () => createRawSnippet(() => ({ render: () => `<p>Body text</p>` }));

describe('Banner', () => {
  it('renders title + body and is warning-toned by default', () => {
    render(Banner, { props: { title: 'Heads up', dataTestid: 'b', children: body() } });
    const box = screen.getByTestId('b');
    expect(box.className).toContain('bg-warning-bg');
    expect(box.className).toContain('border-warning-text');
    expect(screen.getByText('Heads up').className).toContain('text-warning-text');
    expect(screen.getByText('Body text')).toBeTruthy();
  });

  it('renders a dismiss × that calls onDismiss only when onDismiss is provided', async () => {
    const onDismiss = vi.fn();
    render(Banner, { props: { dismissTestid: 'x', onDismiss, children: body() } });
    const x = screen.getByTestId('x');
    await fireEvent.click(x);
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it('omits the × when onDismiss is absent', () => {
    render(Banner, { props: { dismissTestid: 'x', children: body() } });
    expect(screen.queryByTestId('x')).toBeNull();
  });

  it('maps tone to its §1 token family', () => {
    render(Banner, { props: { tone: 'danger', dataTestid: 'b', children: body() } });
    const cls = screen.getByTestId('b').className;
    expect(cls).toContain('bg-danger-bg');
    expect(cls).toContain('border-danger');
  });
});
