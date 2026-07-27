import { render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { describe, expect, it } from 'vitest';
import SidebarSection from '$lib/layout/SidebarSection.svelte';

// A single-root raw snippet used as the section body (mirrors busy-button.test.ts).
const child = () =>
  createRawSnippet(() => ({ render: () => `<button data-testid="section-child">child</button>` }));

describe('SidebarSection', () => {
  it('renders the divider wrapper with the section recipe and the slotted children', () => {
    const { container } = render(SidebarSection, { props: { children: child() } });
    const wrapper = container.querySelector('div');
    const cls = wrapper?.className ?? '';
    expect(cls).toContain('flex');
    expect(cls).toContain('flex-col');
    expect(cls).toContain('gap-1');
    expect(cls).toContain('pt-3');
    expect(cls).toContain('border-t');
    expect(cls).toContain('border-border-subtle');
    expect(screen.getByTestId('section-child')).toBeTruthy();
  });

  it('renders a caps heading carrying the recipe on the matched text element', () => {
    render(SidebarSection, { props: { heading: 'Content', children: child() } });
    // getByText must return the element that itself carries the caps recipe —
    // the heading text is a DIRECT child of the caps div (no <span> wrapper).
    const heading = screen.getByText('Content');
    const cls = heading.className;
    expect(cls).toContain('text-xs');
    expect(cls).toContain('uppercase');
    expect(cls).toContain('tracking-wide');
    expect(cls).toContain('text-muted');
  });

  it('omits the heading entirely when `heading` is undefined', () => {
    render(SidebarSection, { props: { children: child() } });
    expect(screen.queryByText('Content')).toBeNull();
  });

  it('puts data-testid on the heading div when headingTestid is given', () => {
    render(SidebarSection, {
      props: { heading: 'View', headingTestid: 'sidebar-section-view', children: child() },
    });
    const el = screen.getByTestId('sidebar-section-view');
    expect(el.textContent).toContain('View');
    expect(el.className).toContain('uppercase');
  });

  it('sets data-tour on the wrapper when dataTour is given', () => {
    const { container } = render(SidebarSection, {
      props: { heading: 'Account', dataTour: 'account-section', children: child() },
    });
    expect(container.querySelector('[data-tour="account-section"]')).not.toBeNull();
  });

  it('omits data-tour entirely when dataTour is not given (no stray anchor)', () => {
    const { container } = render(SidebarSection, { props: { children: child() } });
    expect(container.querySelector('[data-tour]')).toBeNull();
  });
});
