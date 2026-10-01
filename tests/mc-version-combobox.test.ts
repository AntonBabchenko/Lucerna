import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

// mcVersions rune is read by the combobox; a couple of releases is enough.
vi.mock('$lib/settings/state.svelte', () => ({
  mcVersions: {
    value: [
      { id: '1.20.1', version_type: 'release' },
      { id: '1.21', version_type: 'release' },
    ],
  },
}));

import McVersionCombobox from '$lib/mods/McVersionCombobox.svelte';
import McComboboxInReflowingRow from './fixtures/McComboboxInReflowingRow.svelte';

describe('McVersionCombobox', () => {
  it('opens the listbox on focus and commits a pick when enabled', async () => {
    render(McVersionCombobox, { props: { dataTestid: 'mc', value: '' } });
    const input = screen.getByTestId('mc') as HTMLInputElement;
    expect(input.disabled).toBe(false);
    await fireEvent.focus(input);
    expect(screen.getByRole('listbox')).toBeTruthy();
  });

  it('is inert when disabled — input is disabled and focus does not open the dropdown', async () => {
    render(McVersionCombobox, { props: { dataTestid: 'mc', value: '', disabled: true } });
    const input = screen.getByTestId('mc') as HTMLInputElement;
    expect(input.disabled).toBe(true);
    // Even if a focus event is dispatched, the handler is guarded → no listbox.
    await fireEvent.focus(input);
    expect(screen.queryByRole('listbox')).toBeNull();
    // And typing does not open it either.
    await fireEvent.input(input);
    expect(screen.queryByRole('listbox')).toBeNull();
  });

  // Plan §5e: the list hung 16 px past its input's right edge (`absolute w-32` under a `w-28`
  // input), 4 px past the launcher's 820 px window, where the scroll container cut its border off.
  // It is placed like Select's list now: fixed, kept inside the window by its own width. happy-dom
  // lays nothing out: the list measures 128 px, the input is given its box by the right edge.
  describe('a list that stays inside the window', () => {
    function openAtRightEdge() {
      render(McVersionCombobox, { props: { dataTestid: 'mc', value: '' } });
      const input = screen.getByTestId('mc');
      const left = window.innerWidth - 8 - 112;
      input.getBoundingClientRect = () => ({ top: 190, bottom: 222, left, width: 112 }) as DOMRect;
      return fireEvent.focus(input);
    }

    it('ends inside the window, by its own width', async () => {
      const measured = vi
        .spyOn(HTMLElement.prototype, 'offsetWidth', 'get')
        .mockImplementation(function (this: HTMLElement) {
          return this.getAttribute('role') === 'listbox' ? 128 : 0;
        });
      try {
        await openAtRightEdge();
        const list = screen.getByRole('listbox');
        expect(list.className).toMatch(/\bfixed\b/);
        expect(list.style.left).toBe(`${window.innerWidth - 128 - 8}px`);
        expect(list.style.top).toBe('226px'); // under the input, 4 px apart
      } finally {
        measured.mockRestore();
      }
    });

    // Fixed, it does not follow the input: a scroll of the page around it closes it, as Select's.
    it('closes when the page scrolls', async () => {
      await openAtRightEdge();
      window.dispatchEvent(new Event('scroll'));
      await vi.waitFor(() => expect(screen.queryByRole('listbox')).toBeNull());
    });
  });

  // Fixed (#462), the list escapes the scroll container that cut v0.25.0's `absolute` list off at
  // the bottom of a short page — but it no longer moves with its field. Placed once per open, it
  // stayed behind when a keystroke reflowed the row: the Browse filter bar's first keystroke brings
  // in "Match this instance" or "Clear all", the search box gives up the room, and the field moved
  // left (110 px for "Match this instance" in a 1600 px window) while the list hung under "Sort:".
  // happy-dom lays nothing out: the field's box is a stub the test moves.
  describe('a list that stays under its field while you type', () => {
    const box = (left: number, top: number) =>
      ({ top, bottom: top + 32, left, width: 112 }) as DOMRect;

    it('follows its input when a keystroke moves it', async () => {
      render(McVersionCombobox, { props: { dataTestid: 'mc', value: '' } });
      const input = screen.getByTestId('mc') as HTMLInputElement;
      let at = box(600, 190);
      input.getBoundingClientRect = () => at;
      await fireEvent.input(input, { target: { value: '1' } });
      const list = screen.getByRole('listbox');
      expect(list.style.left).toBe('600px');
      expect(list.style.top).toBe('226px');
      // The row reflowed under the open list and wrapped: the field is further left and lower.
      at = box(490, 230);
      await fireEvent.input(input, { target: { value: '1.2' } });
      await vi.waitFor(() => {
        expect(list.style.left).toBe('490px');
        expect(list.style.top).toBe('266px');
      });
    });

    it('follows its input when the page answers the keystroke in an update of its own', async () => {
      render(McComboboxInReflowingRow);
      const input = screen.getByTestId('mc') as HTMLInputElement;
      // Once the row's answer is in the document, it has moved the field left.
      input.getBoundingClientRect = () => box(screen.queryByTestId('row-answer') ? 490 : 600, 190);
      await fireEvent.focus(input);
      const list = screen.getByRole('listbox');
      expect(list.style.left).toBe('600px');
      await fireEvent.input(input, { target: { value: '1' } });
      await vi.waitFor(() => expect(list.style.left).toBe('490px'));
    });
  });
});
