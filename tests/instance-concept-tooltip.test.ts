import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, test } from 'vitest';
import InstanceConceptTooltip from '../src/lib/onboarding/InstanceConceptTooltip.svelte';

describe('InstanceConceptTooltip', () => {
  test('renders a (?) trigger button', () => {
    render(InstanceConceptTooltip);
    expect(screen.getByRole('button', { name: /what is an instance/i })).toBeTruthy();
  });

  test('clicking the trigger reveals the explanation', async () => {
    render(InstanceConceptTooltip);
    await fireEvent.click(screen.getByRole('button', { name: /what is an instance/i }));
    expect(screen.getByText(/self-contained world/i)).toBeTruthy();
  });

  test('clicking the close button closes the popover', async () => {
    render(InstanceConceptTooltip);
    await fireEvent.click(screen.getByRole('button', { name: /what is an instance/i }));
    expect(screen.getByText(/self-contained world/i)).toBeTruthy();
    // Simulate close via the CloseButton rendered inside the popover.
    const closeBtn = screen.getByRole('button', { name: /close tooltip/i });
    expect(closeBtn).toBeTruthy();
    await fireEvent.click(closeBtn);
    expect(screen.queryByText(/self-contained world/i)).toBeNull();
  });

  test('clicking the trigger while open closes the popover', async () => {
    render(InstanceConceptTooltip);
    const trigger = screen.getByRole('button', { name: /what is an instance/i });
    await fireEvent.click(trigger);
    expect(screen.getByText(/self-contained world/i)).toBeTruthy();
    await fireEvent.click(trigger);
    expect(screen.queryByText(/self-contained world/i)).toBeNull();
  });
});
