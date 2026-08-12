import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import HelpPopover from '../src/lib/ui/HelpPopover.svelte';

describe('HelpPopover paragraphs variant', () => {
  it('renders one <p> per paragraph when `paragraphs` is passed', async () => {
    render(HelpPopover, {
      props: {
        paragraphs: ['First paragraph.', 'Second paragraph.', 'Third paragraph.'],
        triggerAriaLabel: 'What is this?',
        closeAriaLabel: 'Close help',
      },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'What is this?' }));
    const pop = document.getElementById(
      screen.getByRole('button', { name: 'What is this?' }).getAttribute('aria-controls') ?? '',
    );
    expect(pop).not.toBeNull();
    const ps = pop?.querySelectorAll('p') ?? [];
    expect(ps.length).toBe(3);
    expect(ps[0].textContent).toBe('First paragraph.');
    expect(ps[2].textContent).toBe('Third paragraph.');
  });

  it('still renders the single-body variant', async () => {
    render(HelpPopover, {
      props: {
        body: 'Only body.',
        triggerAriaLabel: 'What is this?',
        closeAriaLabel: 'Close help',
      },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'What is this?' }));
    expect(screen.getByText('Only body.')).toBeTruthy();
  });
});
