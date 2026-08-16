import { fireEvent, render } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('$lib/ipc/bindings', () => ({
  commands: { instanceJournalRead: vi.fn(), instanceJournalClear: vi.fn() },
}));

import { commands } from '$lib/ipc/bindings';
import JournalCrashContext from '$lib/journal/JournalCrashContext.svelte';
import JournalPanel from '$lib/journal/JournalPanel.svelte';
import { hideTooltip, tooltipState } from '$lib/ui/tooltip/tooltip-controller.svelte';
import { dismissTooltip, revealTooltip } from './test-utils/reveal-tooltip';

const NOW = Date.UTC(2026, 6, 30, 12, 0, 0);
const HOUR = 60 * 60 * 1000;

function contentEntry(atMs: number, subject: string, action = 'mod_installed') {
  return {
    at_unix_ms: atMs,
    event: {
      kind: 'content',
      action,
      subject,
      from_version: null,
      to_version: null,
      affected: null,
    },
  };
}

function launchEntry(atMs: number, outcome: string, logPath: string | null = null) {
  return {
    at_unix_ms: atMs,
    event: { kind: 'launch', outcome, exit_code: -1, duration_seconds: 120, log_path: logPath },
  };
}

function mockJournal(entries: unknown[]) {
  vi.mocked(commands.instanceJournalRead).mockResolvedValue({
    status: 'ok',
    data: entries,
    // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
  } as any);
}

afterEach(() => {
  vi.clearAllMocks();
});

describe('JournalPanel', () => {
  it('shows the empty state when the instance has no recorded activity', async () => {
    mockJournal([]);
    const { findByTestId } = render(JournalPanel, { props: { instanceId: 'inst-1' } });
    expect(await findByTestId('journal-empty')).toBeTruthy();
  });

  it('renders a row for both event shapes', async () => {
    mockJournal([launchEntry(NOW, 'crashed'), contentEntry(NOW - HOUR, 'Sodium')]);
    const { findAllByTestId } = render(JournalPanel, { props: { instanceId: 'inst-1' } });
    const rows = await findAllByTestId('journal-row');
    expect(rows).toHaveLength(2);
    const text = rows.map((r) => r.textContent ?? '').join(' ');
    expect(text).toContain('Sodium');
    expect(text).toContain('Crashed');
  });

  it('surfaces a read failure instead of pretending the history is empty', async () => {
    vi.mocked(commands.instanceJournalRead).mockResolvedValue({
      status: 'error',
      error: { kind: 'io', path: 'p', details: 'nope' },
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    const { findByTestId, queryByTestId } = render(JournalPanel, {
      props: { instanceId: 'inst-1' },
    });
    expect(await findByTestId('journal-error')).toBeTruthy();
    expect(queryByTestId('journal-empty')).toBeNull();
  });

  it('the Launches filter hides content rows', async () => {
    mockJournal([launchEntry(NOW, 'ok'), contentEntry(NOW - HOUR, 'Sodium')]);
    const { findAllByTestId, getByTestId } = render(JournalPanel, {
      props: { instanceId: 'inst-1' },
    });
    expect(await findAllByTestId('journal-row')).toHaveLength(2);

    await fireEvent.click(getByTestId('journal-filter-launches'));

    const rows = await findAllByTestId('journal-row');
    expect(rows).toHaveLength(1);
    expect(rows[0].textContent ?? '').not.toContain('Sodium');
  });

  it('a launch row with a captured log offers Open log and reports the path', async () => {
    mockJournal([launchEntry(NOW, 'crashed', 'C:/i/logs/run.log')]);
    const opened: string[] = [];
    const { findByRole } = render(JournalPanel, {
      props: { instanceId: 'inst-1', onOpenLog: (p: string) => opened.push(p) },
    });
    const btn = await findByRole('button', { name: /open log/i });
    await fireEvent.click(btn);
    expect(opened).toEqual(['C:/i/logs/run.log']);
  });

  it('clear is disabled while the history is empty', async () => {
    mockJournal([]);
    const { findByTestId } = render(JournalPanel, { props: { instanceId: 'inst-1' } });
    const btn = (await findByTestId('journal-clear')) as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
  });

  it('clearing empties the list only after the confirm', async () => {
    mockJournal([contentEntry(NOW, 'Sodium')]);
    vi.mocked(commands.instanceJournalClear).mockResolvedValue({
      status: 'ok',
      data: null,
      // biome-ignore lint/suspicious/noExplicitAny: mocked IPC envelope
    } as any);
    const { findAllByTestId, findByTestId, getByTestId } = render(JournalPanel, {
      props: { instanceId: 'inst-1' },
    });
    expect(await findAllByTestId('journal-row')).toHaveLength(1);

    await fireEvent.click(getByTestId('journal-clear'));
    // Still there — the dialog is open, nothing has been deleted yet.
    expect(await findAllByTestId('journal-row')).toHaveLength(1);
    expect(commands.instanceJournalClear).not.toHaveBeenCalled();

    await fireEvent.click(getByTestId('journal-clear-confirm'));
    expect(commands.instanceJournalClear).toHaveBeenCalledWith('inst-1');
    expect(await findByTestId('journal-empty')).toBeTruthy();
  });

  it('reveals the full subject only when the row clips it', async () => {
    // DESIGN.md §5 bans native title=. The overflow tooltip is the prescribed
    // replacement (KeyEditRow's truncated key); `whenOverflowing` consults
    // scrollWidth > clientWidth, which happy-dom reports as 0 > 0 = false, so
    // the two cases are driven by stubbing those two properties on the node.
    const long = 'A mod with a very long display name indeed';
    mockJournal([contentEntry(NOW, long)]);
    const { findAllByTestId } = render(JournalPanel, { props: { instanceId: 'inst-1' } });
    const [row] = await findAllByTestId('journal-row');
    const subject = row.querySelector('.truncate') as HTMLElement;

    expect(subject.getAttribute('title')).toBeNull();

    Object.defineProperty(subject, 'scrollWidth', { value: 400, configurable: true });
    Object.defineProperty(subject, 'clientWidth', { value: 120, configurable: true });
    revealTooltip(subject);
    expect(tooltipState.visible).toBe(true);
    expect(tooltipState.text).toBe(long);
    dismissTooltip(subject);

    Object.defineProperty(subject, 'scrollWidth', { value: 120, configurable: true });
    revealTooltip(subject);
    expect(tooltipState.visible).toBe(false);
    hideTooltip();
  });
});

describe('JournalCrashContext', () => {
  it('renders nothing when no content change falls inside the window', async () => {
    // A launch entry is not evidence, and a 3-day-old install is out of window.
    mockJournal([launchEntry(NOW - HOUR, 'crashed'), contentEntry(NOW - 72 * HOUR, 'Ancient')]);
    const { queryByTestId } = render(JournalCrashContext, {
      props: { instanceId: 'inst-1', anchorMs: NOW },
    });
    await vi.waitFor(() => expect(commands.instanceJournalRead).toHaveBeenCalled());
    expect(queryByTestId('journal-crash-context')).toBeNull();
  });

  it('names the change that landed shortly before the log', async () => {
    mockJournal([contentEntry(NOW - 2 * HOUR, 'Create')]);
    const { findByTestId } = render(JournalCrashContext, {
      props: { instanceId: 'inst-1', anchorMs: NOW },
    });
    const strip = await findByTestId('journal-crash-context');
    expect(strip.textContent ?? '').toContain('Create');
  });

  it('renders nothing without an anchor time', async () => {
    mockJournal([contentEntry(NOW - HOUR, 'Create')]);
    const { queryByTestId } = render(JournalCrashContext, {
      props: { instanceId: 'inst-1', anchorMs: null },
    });
    await vi.waitFor(() => expect(commands.instanceJournalRead).toHaveBeenCalled());
    expect(queryByTestId('journal-crash-context')).toBeNull();
  });
});
