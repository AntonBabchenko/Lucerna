import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import RenderedBody from '$lib/ui/RenderedBody.svelte';

describe('RenderedBody', () => {
  it('injects sanitized html', () => {
    const { container } = render(RenderedBody, {
      props: { html: '<h1>Title</h1><p>body</p>' },
    });
    expect(container.querySelector('h1')?.textContent).toBe('Title');
    expect(container.querySelector('p')?.textContent).toBe('body');
  });

  it('renders an empty container for empty html', () => {
    const { container } = render(RenderedBody, { props: { html: '' } });
    expect(container.querySelector('.prose-body')?.textContent).toBe('');
  });
});
