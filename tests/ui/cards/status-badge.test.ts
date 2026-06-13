import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
import { createRawSnippet } from 'svelte';

function text(label: string) {
  return createRawSnippet(() => ({ render: () => `<span>${label}</span>` }));
}

describe('StatusBadge', () => {
  it('renders the label text', () => {
    render(StatusBadge, { props: { variant: 'success', children: text('enabled') } });
    expect(screen.getByText('enabled')).toBeTruthy();
  });

  it('applies the variant colour classes', () => {
    const { container } = render(StatusBadge, {
      props: { variant: 'warning', children: text('update') },
    });
    const pill = container.querySelector('span');
    expect(pill?.className).toContain('bg-warning-bg');
    expect(pill?.className).toContain('text-warning-text');
  });
});
