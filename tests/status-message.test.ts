import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import StatusMessage from '$lib/ui/StatusMessage.svelte';

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

  it('honours an explicit live override', () => {
    render(StatusMessage, { props: { message: 'x', tone: 'danger', live: 'polite' } });
    expect(screen.getByRole('status')).toBeTruthy();
    expect(screen.queryByRole('alert')).toBeNull();
  });
});
