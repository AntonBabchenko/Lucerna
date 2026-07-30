// Saved-server status in the servers dialog. The two things worth pinning are
// honesty properties, not layout: with the permission off the dialog SAYS so
// and shows no per-row status at all, and a server that did not reply is
// reported as "no answer" rather than as a claim that it is offline (we have no
// SRV lookup, so silence is genuinely "we could not tell").

import { fireEvent, render, screen } from '@testing-library/svelte';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { locale } from '$lib/i18n';
import QuickJoinDialog from '$lib/worlds/QuickJoinDialog.svelte';
import { formatPingChip } from '$lib/worlds/server-ping';

const saved = [
  { name: 'SMP', address: 'play.example.net' },
  { name: 'Anom', address: 'mc.x:25566' },
];

function props(over: Record<string, unknown> = {}) {
  return {
    open: true,
    savedServers: saved,
    connectDisabledReason: null,
    onConnect: vi.fn(),
    onSave: vi.fn().mockResolvedValue(true),
    onSaveAndConnect: vi.fn().mockResolvedValue(true),
    onDelete: vi.fn(),
    onClose: vi.fn(),
    ...over,
  };
}

describe('formatPingChip', () => {
  it('joins the parts the server actually reported', () => {
    expect(
      formatPingChip({
        kind: 'online',
        players_online: 7,
        players_max: 40,
        version_name: '1.21.4',
        motd: null,
        latency_ms: 41,
      }),
    ).toBe('7/40 · 1.21.4 · 41 ms');
  });

  it('omits missing fields instead of inventing placeholders', () => {
    // Every SLP field is optional on the wire; a "?" would read as data.
    expect(
      formatPingChip({
        kind: 'online',
        players_online: null,
        players_max: null,
        version_name: null,
        motd: null,
        latency_ms: 12,
      }),
    ).toBe('12 ms');
  });

  it('shows a bare online count when max is missing', () => {
    expect(
      formatPingChip({
        kind: 'online',
        players_online: 3,
        players_max: null,
        version_name: null,
        motd: null,
        latency_ms: 9,
      }),
    ).toBe('3 · 9 ms');
  });
});

describe('QuickJoinDialog server status', () => {
  beforeAll(() => locale.set('en'));

  it('with the permission off, says so and shows no status chips', () => {
    render(QuickJoinDialog, props({ pingEnabled: false }));
    expect(screen.getByTestId('ping-disabled-notice')).toBeTruthy();
    expect(screen.queryAllByTestId('ping-chip')).toHaveLength(0);
    expect(screen.queryByRole('button', { name: 'Refresh status' })).toBeNull();
  });

  it('the off-notice offers a route to the setting', async () => {
    const onOpenPingSetting = vi.fn();
    render(QuickJoinDialog, props({ pingEnabled: false, onOpenPingSetting }));
    await fireEvent.click(screen.getByRole('button', { name: 'Enable in Settings' }));
    expect(onOpenPingSetting).toHaveBeenCalled();
  });

  it('renders players, version and response time once a ping lands', () => {
    render(
      QuickJoinDialog,
      props({
        pingEnabled: true,
        pingStates: {
          'play.example.net': {
            kind: 'online',
            players_online: 7,
            players_max: 40,
            version_name: '1.21.4',
            motd: 'Welcome',
            latency_ms: 41,
          },
        },
      }),
    );
    expect(screen.getByText('7/40 · 1.21.4 · 41 ms')).toBeTruthy();
  });

  it('says "no answer" rather than claiming the server is offline', () => {
    render(
      QuickJoinDialog,
      props({ pingEnabled: true, pingStates: { 'play.example.net': { kind: 'no_answer' } } }),
    );
    expect(screen.getByText('No answer')).toBeTruthy();
  });

  it('shows a checking state while a ping is in flight', () => {
    render(
      QuickJoinDialog,
      props({ pingEnabled: true, pingStates: { 'play.example.net': 'pending' } }),
    );
    expect(screen.getByText('Checking…')).toBeTruthy();
  });

  it('refresh is offered when the permission is on, and reports back', async () => {
    const onRefreshPings = vi.fn();
    render(QuickJoinDialog, props({ pingEnabled: true, onRefreshPings }));
    await fireEvent.click(screen.getByRole('button', { name: 'Refresh status' }));
    expect(onRefreshPings).toHaveBeenCalled();
  });

  it('refresh is disabled while a sweep is still in flight', () => {
    render(
      QuickJoinDialog,
      props({ pingEnabled: true, pingStates: { 'play.example.net': 'pending' } }),
    );
    const refresh = screen.getByRole('button', { name: 'Refresh status' }) as HTMLButtonElement;
    expect(refresh.disabled).toBe(true);
  });
});
