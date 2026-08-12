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

  it('renders duplicate paragraph text without crashing (index-keyed each)', async () => {
    render(HelpPopover, {
      props: {
        paragraphs: ['Same text.', 'Same text.'],
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
    expect(ps.length).toBe(2);
    expect(ps[0].textContent).toBe('Same text.');
    expect(ps[1].textContent).toBe('Same text.');
  });

  // The one-sentence helper is not a separate template branch any more — it is
  // a single-element array. This pins that it still renders exactly one <p>,
  // carrying the close-button clearance (`pr-6`) the old `body` branch had.
  it('renders a single paragraph, with the close-button clearance on the <p>', async () => {
    render(HelpPopover, {
      props: {
        paragraphs: ['Only body.'],
        triggerAriaLabel: 'What is this?',
        closeAriaLabel: 'Close help',
      },
    });
    await fireEvent.click(screen.getByRole('button', { name: 'What is this?' }));
    const pop = document.getElementById(
      screen.getByRole('button', { name: 'What is this?' }).getAttribute('aria-controls') ?? '',
    );
    const ps = pop?.querySelectorAll('p') ?? [];
    expect(ps.length).toBe(1);
    expect(ps[0].textContent).toBe('Only body.');
    expect(ps[0].className).toContain('pr-6');
  });
});
