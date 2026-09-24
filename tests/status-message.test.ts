import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import StatusMessage from '$lib/ui/StatusMessage.svelte';
import StatusMessageBound from './fixtures/StatusMessageBound.svelte';

describe('StatusMessage', () => {
  it('uses role=alert for danger (assertive by default)', () => {
    render(StatusMessage, { props: { message: 'Boom', tone: 'danger' } });
    const region = screen.getByRole('alert');
    expect(region.textContent).toContain('Boom');
  });

  it('uses role=status for warning/info (polite by default)', () => {
    render(StatusMessage, { props: { message: 'Heads up', tone: 'warning' } });
    expect(screen.getByRole('status').textContent).toContain('Heads up');
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('keeps the live region present (but empty) when message is null', () => {
    render(StatusMessage, { props: { message: null, tone: 'danger' } });
    const region = screen.getByRole('alert');
    expect(region).toBeTruthy();
    expect(region.textContent?.trim()).toBe('');
  });

  it('reserves a line of height when reserveSpace is set', () => {
    render(StatusMessage, { props: { message: null, reserveSpace: true } });
    expect(screen.getByRole('alert').className).toContain('min-h-4');
  });

  // An idle region is an empty box. In flow it would still be a flex/grid item
  // and cost its container one extra gap (tests/no-idle-live-region-box.test.ts).
  it('keeps an idle region in the accessibility tree but out of flow', () => {
    render(StatusMessage, { props: { message: null, tone: 'danger' } });
    const region = screen.getByRole('alert');
    expect(region.classList.contains('absolute')).toBe(true);
    // Out of flow, never hidden: a hidden region does not announce what arrives.
    expect(region.hidden).toBe(false);
    expect(region.getAttribute('aria-hidden')).toBeNull();
    expect(region.className).not.toMatch(/\b(hidden|invisible|sr-only)\b/);
    expect(region.getAttribute('aria-atomic')).toBe('true');
  });

  it('the same node takes the message and goes back into flow, then out again', async () => {
    const { rerender } = render(StatusMessage, { props: { message: null, tone: 'info' } });
    const region = screen.getByRole('status');
    await rerender({ message: 'Saved', tone: 'info' });
    // Same node: an announcement depends on the region already being there.
    expect(screen.getByRole('status')).toBe(region);
    expect(region.classList.contains('absolute')).toBe(false);
    expect(region.textContent).toContain('Saved');
    await rerender({ message: null, tone: 'info' });
    expect(region.classList.contains('absolute')).toBe(true);
  });

  it('a reserved line stays in flow even while idle', () => {
    render(StatusMessage, { props: { message: null, reserveSpace: true } });
    const region = screen.getByRole('alert');
    expect(region.className).toContain('min-h-4');
    expect(region.classList.contains('absolute')).toBe(false);
  });

  it('puts the test hook on the region itself, so nothing has to wrap it', () => {
    render(StatusMessage, {
      props: { message: null, tone: 'danger', dataTestid: 'save-failure-x' },
    });
    expect(screen.getByTestId('save-failure-x')).toBe(screen.getByRole('alert'));
  });

  it('binds the region element for a caller that scrolls it into view', async () => {
    render(StatusMessageBound, { props: { message: 'Boom' } });
    await vi.waitFor(() => expect(screen.getByTestId('bound').textContent).toBe('alert'));
  });

  it('honours an explicit live override', () => {
    render(StatusMessage, { props: { message: 'x', tone: 'danger', live: 'polite' } });
    expect(screen.getByRole('status')).toBeTruthy();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('renders the success tone as text-success under role=status', () => {
    render(StatusMessage, { props: { message: 'Saved', tone: 'success' } });
    const region = screen.getByRole('status');
    expect(region.textContent).toContain('Saved');
    expect(region.querySelector('p')?.className).toContain('text-success');
  });
});
