import { fireEvent, render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { createRawSnippet } from 'svelte';
import { describe, expect, it, vi } from 'vitest';
import BusyButton from '../src/lib/ui/BusyButton.svelte';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import { hoverTooltip } from './test-utils/hover-tooltip';

const label = (text: string) => createRawSnippet(() => ({ render: () => `<span>${text}</span>` }));

const labelSnippet = createRawSnippet(() => ({ render: () => `<span>Save</span>` }));

describe('BusyButton — prop forwarding', () => {
  it('forwards data-testid and data-tour onto the button', () => {
    render(BusyButton, {
      props: { children: labelSnippet, 'data-testid': 'save-btn', 'data-tour': 'x' },
    });
    const btn = screen.getByTestId('save-btn');
    expect(btn.tagName).toBe('BUTTON');
    expect(btn.getAttribute('data-tour')).toBe('x');
  });

  it('shows a spinner and disables when busy', () => {
    render(BusyButton, {
      props: { children: labelSnippet, busy: true, 'data-testid': 'save-btn' },
    });
    const btn = screen.getByTestId('save-btn');
    expect(btn.getAttribute('aria-busy')).toBe('true');
    expect((btn as HTMLButtonElement).disabled).toBe(true);
    expect(btn.querySelector('[role="status"]')).not.toBeNull();
    expect(btn.textContent).toContain('Save');
  });

  it('routes `title` through the tooltip layer, never a native title=', () => {
    // DESIGN.md §5 bans native title=. The prop survives (rather than being
    // deleted as unused) because destructuring it is what keeps a caller's
    // title out of `...rest` and off the DOM — a hole the source guard in
    // tests/no-native-title.test.ts cannot see.
    render(BusyButton, {
      props: { children: labelSnippet, title: 'Applies to every world', 'data-testid': 'b' },
    });
    const btn = screen.getByTestId('b');
    expect(btn.getAttribute('title')).toBeNull();
    hoverTooltip(btn);
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe('Applies to every world');
    hideTooltip();
  });

  it('adds no tooltip when no title is given', () => {
    render(BusyButton, { props: { children: labelSnippet, 'data-testid': 'b' } });
    const btn = screen.getByTestId('b');
    hoverTooltip(btn);
    expect(tooltipState.visible).toBe(false);
    hideTooltip();
  });
});

describe('BusyButton', () => {
  it('renders the label and fires onclick when idle', async () => {
    const onclick = vi.fn();
    const { getByRole } = render(BusyButton, {
      props: { onclick, children: label('Install') },
    });
    const btn = getByRole('button');
    expect(btn.textContent).toContain('Install');
    expect(btn.hasAttribute('disabled')).toBe(false);
    expect(btn.querySelector('[role="status"]')).toBeNull();
    await fireEvent.click(btn);
    expect(onclick).toHaveBeenCalledTimes(1);
  });

  it('when busy: disabled, aria-busy, spinner AND label both present', async () => {
    const onclick = vi.fn();
    const { getByRole } = render(BusyButton, {
      props: { busy: true, onclick, children: label('Install') },
    });
    const btn = getByRole('button');
    expect(btn.hasAttribute('disabled')).toBe(true);
    expect(btn.getAttribute('aria-busy')).toBe('true');
    expect(btn.querySelector('[role="status"]')).not.toBeNull();
    expect(btn.textContent).toContain('Install');
    // userEvent respects the disabled attribute; fireEvent would not.
    await userEvent.setup().click(btn);
    expect(onclick).not.toHaveBeenCalled();
  });

  it('respects independent `disabled` without showing a spinner', async () => {
    const onclick = vi.fn();
    const { getByRole } = render(BusyButton, {
      props: { disabled: true, onclick, children: label('Install') },
    });
    const btn = getByRole('button');
    expect(btn.hasAttribute('disabled')).toBe(true);
    expect(btn.getAttribute('aria-busy')).toBe('false');
    expect(btn.querySelector('[role="status"]')).toBeNull();
    // userEvent respects the disabled attribute; fireEvent would not.
    await userEvent.setup().click(btn);
    expect(onclick).not.toHaveBeenCalled();
  });

  it('applies spinnerClass to the rendered spinner element', () => {
    const { getByRole } = render(BusyButton, {
      props: { busy: true, spinnerClass: 'text-red-500', children: label('Install') },
    });
    const status = getByRole('button').querySelector('[role="status"]');
    expect(status).not.toBeNull();
    expect(status?.className).toContain('text-red-500');
  });

  it('forwards the provided class to the button', () => {
    const { getByRole } = render(BusyButton, {
      props: { class: 'btn-primary btn-xs', children: label('Go') },
    });
    expect(getByRole('button').className).toContain('btn-primary');
  });
});
