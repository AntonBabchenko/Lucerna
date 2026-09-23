// tests/help-panel.test.ts
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeEach, describe, expect, test } from 'vitest';
import { explanationState } from '../src/lib/onboarding/explanation-level.svelte';
import { tourState } from '../src/lib/onboarding/state.svelte';
import HelpPanel from '../src/lib/settings/HelpPanel.svelte';
import { settingsOpen, settingsSearchFocus } from '../src/lib/settings/state.svelte';
import { describedText } from './test-utils/aria';

beforeEach(() => {
  tourState.active = false;
  tourState.currentStep = 0;
  explanationState.level = 'basic';
  settingsOpen.value = { tab: 'help' };
});

describe('HelpPanel', () => {
  test('renders the tip-level selector', () => {
    render(HelpPanel);
    expect(screen.getByTestId('tip-level-select')).toBeTruthy();
  });

  test('renders the Replay the tour button', () => {
    render(HelpPanel);
    expect(screen.getByRole('button', { name: /replay the tour/i })).toBeTruthy();
  });

  test('clicking Replay activates the tour AND closes Settings', async () => {
    render(HelpPanel);
    await fireEvent.click(screen.getByRole('button', { name: /replay the tour/i }));
    expect(tourState.active).toBe(true);
    expect(tourState.currentStep).toBe(0);
    expect(settingsOpen.value).toBe(null);
  });

  test('clicking Replay drops a pending jump with the modal — nothing flashes on the next open', async () => {
    // Not one of Help's own anchors: those would be consumed on mount.
    settingsSearchFocus.value = 'storage.cache';
    render(HelpPanel);
    await fireEvent.click(screen.getByRole('button', { name: /replay the tour/i }));
    expect(settingsOpen.value).toBe(null);
    expect(settingsSearchFocus.value).toBe(null);
  });

  test('the tip level is a group of Basic / Advanced whose one hint follows the selection', () => {
    const hintOf = (g: HTMLElement) =>
      document.getElementById(g.getAttribute('aria-describedby') as string)?.textContent?.trim();

    const first = render(HelpPanel);
    const group = screen.getByTestId('tip-level-select');
    expect(group.getAttribute('role')).toBe('group');
    expect(within(group).getByRole('button', { name: 'Basic' }).getAttribute('aria-pressed')).toBe(
      'true',
    );
    expect(
      within(group).getByRole('button', { name: 'Advanced' }).getAttribute('aria-pressed'),
    ).toBe('false');
    expect(hintOf(group)).toBe('simple and clear');
    first.unmount();

    explanationState.level = 'advanced';
    render(HelpPanel);
    expect(hintOf(screen.getByTestId('tip-level-select'))).toBe('technical details');
  });

  test("Replay's description sits under the button, and the button does not wrap", () => {
    render(HelpPanel);
    const btn = screen.getByRole('button', { name: /replay the tour/i });
    const desc = screen.getByText(/Show the tour again/);
    expect(btn.className).toContain('shrink-0');
    const column = btn.parentElement as HTMLElement;
    expect(column.className).toContain('flex-col');
    expect(column.contains(desc)).toBe(true);
    expect(btn.compareDocumentPosition(desc) & Node.DOCUMENT_POSITION_FOLLOWING).not.toBe(0);
  });

  test('the tip-level group and the Replay button are described by their helper lines', () => {
    render(HelpPanel);
    // The tips hint follows the selection (10a); either sentence is the link's target.
    expect(describedText(screen.getByTestId('tip-level-select'))).toMatch(
      /simple and clear|technical details/,
    );
    expect(describedText(screen.getByRole('button', { name: /replay the tour/i }))).toContain(
      'per-screen tours',
    );
  });

  test('the page names one concept one way: Tour and Explanations', () => {
    render(HelpPanel);
    // Five names for two concepts was the defect: "Tips" meant both the detail
    // level and the per-screen tours, and the tour itself answered to
    // "Onboarding", "welcome tour" and "Replay tours".
    expect(screen.getByRole('heading', { level: 3, name: 'Tour' })).toBeTruthy();
    expect(screen.getByRole('heading', { level: 3, name: 'Explanations' })).toBeTruthy();
    expect(screen.getByText('Detail level')).toBeTruthy();
    expect(screen.queryByText(/onboarding/i)).toBeNull();
  });
});
