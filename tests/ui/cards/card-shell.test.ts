import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import CardShell from '$lib/ui/cards/CardShell.svelte';
import { createRawSnippet } from 'svelte';

const body = createRawSnippet(() => ({ render: () => `<div data-testid="body">x</div>` }));

describe('CardShell', () => {
  it('renders an accent strip element coloured by the accent tone', () => {
    const { container } = render(CardShell, {
      props: { variant: 'row', accent: 'warning', children: body },
    });
    const strip = container.querySelector('[data-card-accent]');
    expect(strip?.className).toContain('bg-warning-text');
  });

  it('dims the shell when dim={true}', () => {
    const { container } = render(CardShell, {
      props: { variant: 'row', dim: true, children: body },
    });
    expect(container.querySelector('[data-card-shell]')?.className).toContain('opacity-60');
  });

  it('renders the children', () => {
    const { getByTestId } = render(CardShell, { props: { variant: 'tile', children: body } });
    expect(getByTestId('body')).toBeTruthy();
  });
});
