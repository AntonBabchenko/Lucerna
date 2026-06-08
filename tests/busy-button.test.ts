import { fireEvent, render } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import { describe, expect, it, vi } from 'vitest';
import BusyButton from '../src/lib/ui/BusyButton.svelte';

const label = (text: string) => createRawSnippet(() => ({ render: () => `<span>${text}</span>` }));

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
    await fireEvent.click(btn);
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
    await fireEvent.click(btn);
    expect(onclick).not.toHaveBeenCalled();
  });

  it('forwards the provided class to the button', () => {
    const { getByRole } = render(BusyButton, {
      props: { class: 'btn-primary btn-xs', children: label('Go') },
    });
    expect(getByRole('button').className).toContain('btn-primary');
  });
});
