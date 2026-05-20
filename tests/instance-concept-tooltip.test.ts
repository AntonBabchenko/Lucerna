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

  test('clicking outside closes the popover', async () => {
    const { container } = render(InstanceConceptTooltip);
    await fireEvent.click(screen.getByRole('button', { name: /what is an instance/i }));
    expect(screen.getByText(/self-contained world/i)).toBeTruthy();
    // Simulate outside click via the backdrop button rendered when open.
    const backdrop = container.querySelector('[aria-label="Close instance concept tooltip"]');
    expect(backdrop).toBeTruthy();
    await fireEvent.click(backdrop as HTMLElement);
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
