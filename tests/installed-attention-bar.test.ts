import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import AttentionBar from '$lib/mods/installed/AttentionBar.svelte';

describe('AttentionBar', () => {
  it('renders nothing when clean', () => {
    const { container } = render(AttentionBar, {
      props: { issues: 0, updates: 0, onShowIssues: vi.fn(), onShowUpdates: vi.fn() },
    });
    expect(container.textContent?.trim()).toBe('');
  });

  it('shows issue + update counts and fires the right callbacks', async () => {
    const onShowIssues = vi.fn();
    const onShowUpdates = vi.fn();
    render(AttentionBar, { props: { issues: 2, updates: 3, onShowIssues, onShowUpdates } });
    await fireEvent.click(screen.getByText(/dependency problems/));
    expect(onShowIssues).toHaveBeenCalled();
    await fireEvent.click(screen.getByText(/updates available/));
    expect(onShowUpdates).toHaveBeenCalled();
  });

  it('shows only the issues segment when there are no updates', () => {
    render(AttentionBar, {
      props: { issues: 1, updates: 0, onShowIssues: vi.fn(), onShowUpdates: vi.fn() },
    });
    expect(screen.getByText(/dependency problems/)).toBeTruthy();
    expect(screen.queryByText(/updates available/)).toBeNull();
  });
});
