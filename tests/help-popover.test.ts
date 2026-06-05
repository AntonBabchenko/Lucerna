import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import HelpPopover from '$lib/ui/HelpPopover.svelte';

const props = {
  body: 'Explains the thing in one sentence.',
  triggerAriaLabel: 'What is this?',
  closeAriaLabel: 'Close tooltip',
};

describe('HelpPopover', () => {
  it('renders a (?) trigger and reveals/hides the body on toggle', async () => {
    render(HelpPopover, { props });
    const trigger = screen.getByRole('button', { name: /what is this/i });
    expect(screen.queryByText(/explains the thing/i)).toBeNull();
    await fireEvent.click(trigger);
    expect(screen.getByText(/explains the thing/i)).toBeTruthy();
    await fireEvent.click(trigger);
    expect(screen.queryByText(/explains the thing/i)).toBeNull();
  });

  it('closes via the close button', async () => {
    render(HelpPopover, { props });
    await fireEvent.click(screen.getByRole('button', { name: /what is this/i }));
    await fireEvent.click(screen.getByRole('button', { name: /close tooltip/i }));
    expect(screen.queryByText(/explains the thing/i)).toBeNull();
  });

  // Regression: the trigger must NOT carry a persistent z-50 — a static high
  // z-index made the "(?)" glyph paint over modals layered above its host. It is
  // only elevated while the popover is open (to sit above its own backdrop).
  it('elevates the trigger (z-50) only while open', async () => {
    render(HelpPopover, { props });
    const trigger = screen.getByRole('button', { name: /what is this/i });
    expect(trigger.className).not.toContain('z-50');
    await fireEvent.click(trigger);
    expect(trigger.className).toContain('z-50');
    await fireEvent.click(trigger);
    expect(trigger.className).not.toContain('z-50');
  });
});
