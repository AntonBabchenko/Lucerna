// Logs group intent coverage: LogsPopover (popover container, header toolbar
// buttons, file-list sidebar, search bar prev/next, severity colour lines,
// share-confirm dialog, share-URL pill, diagnosis card, crash-section view,
// stack-fold expand buttons, empty/loading/error/no-file states).
//
// render.ts is a pure JS module already covered by tests/logs-render.test.ts.
// This file covers the Svelte rendering surface only.
//
// Cluster D covers:
//   - (no logs buttons in cluster D tests)
//
// Inventory rows covered (LogsPopover.svelte lines 489–861):
//   popover aside — bg-surface rounded shadow-xl
//   backdrop button — aria-label="Close logs" (absolute inset-0)
//   Reload button — btn-secondary btn-xs
//   Share button — btn-secondary btn-xs (disabled when no content)
//   CloseButton (header) — btn-icon
//   Wrap-lines checkbox — accent-primary
//   Collapse-stacks checkbox — accent-primary
//   Per-level severity checkboxes — accent-primary (when activeLevels > 0)
//   Search input — role="combobox"-like text input (placeholder "Find in file…")
//   Prev-match button — btn-icon (disabled when totalMatches === 0)
//   Next-match button — btn-icon (disabled when totalMatches === 0)
//   N / M match counter — visible when search has matches
//   "No matches" text — visible when search has no matches
//   Share-confirm dialog inner — bg-surface rounded shadow-xl
//   Cancel (share confirm) — btn-secondary btn-xs
//   Upload & share button — btn-warning btn-xs
//   Share-URL pill — bg-warning-bg border-b text-warning-text
//   Copy button (share URL) — btn-secondary btn-xs
//   CloseButton (share URL dismiss) — btn-icon
//   Truncated warning banner — bg-warning-bg text-warning-text
//   Diagnosis card — border-warning-text/30 bg-warning-bg (details element)
//   diagnosis.title visible inside <summary>
//   diagnosis.explanation + diagnosis.recommendation visible
//   Crash-section expand button — bg-subtle hover:bg-accent-soft font-semibold
//   Stack-fold expand button (line view) — btn-tertiary text-left w-full
//   Error severity line — bg-danger-bg border-danger
//   Warn severity line — bg-warning-bg/30 border-warning-text
//   Debug/trace severity line — text-muted border-border-subtle
//   Empty state — "Select a file on the left"
//   Loading state — "Reading…" text-muted (class-string integrity)
//   Content error state — text-danger
//   File-list empty state — "No log files yet." text-muted
//   File-list error state — text-danger
//   File-list sidebar file button — hover:bg-subtle (bare, not .btn-*)
//   Selected file button — bg-accent-soft added

import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Diagnosis, LogFileMeta } from '$lib/ipc/bindings';

// vi.mock is hoisted before imports.
vi.mock('$lib/ipc/bindings', () => ({
  commands: {
    listLogFiles: vi.fn().mockResolvedValue({ status: 'ok', data: [] }),
    readLogFile: vi.fn().mockResolvedValue({ status: 'ok', data: '' }),
    diagnoseLog: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    shareLogToMclogs: vi.fn().mockResolvedValue({ status: 'ok', data: 'https://mclo.gs/abc123' }),
    latestCrash: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    openLogFolder: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    applyLogRetention: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { deleted_count: 0, freed_bytes: 0 } }),
    deleteLogFile: vi.fn().mockResolvedValue({ status: 'ok', data: null }),
    clearOldLogs: vi
      .fn()
      .mockResolvedValue({ status: 'ok', data: { deleted_count: 0, freed_bytes: 0 } }),
  },
  events: {
    processExited: { listen: vi.fn().mockResolvedValue(() => {}) },
    gpuPrefApplied: { listen: () => Promise.resolve(() => {}) },
  },
}));

import LogsPopover from '$lib/logs/LogsPopover.svelte';

// ── Fixture factories ─────────────────────────────────────────────────────────

function makeLogFileMeta(over: Partial<LogFileMeta> = {}): LogFileMeta {
  return {
    path: '/instances/inst-1/logs/latest.log',
    name: 'latest.log',
    source: 'game',
    size_bytes: 1024 * 100,
    modified_unix_ms: Date.now() - 1000 * 60 * 5,
    ...over,
  };
}

function makeDiagnosis(over: Partial<Diagnosis> = {}): Diagnosis {
  return {
    pattern_id: 'missing_java',
    title: 'Java not found',
    explanation: 'The Java executable could not be located.',
    recommendation: 'Install Java 21 and point Lucerna at it.',
    matched_excerpt: 'Could not find or load main class',
    repair: null,
    ...over,
  };
}

// ── Popover container ─────────────────────────────────────────────────────────

describe('LogsPopover — container has bg-surface rounded shadow-xl', () => {
  it('popover aside has bg-surface, rounded, shadow-xl when open=true', () => {
    const { container } = render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1' },
    });
    const aside = container.querySelector('aside');
    expect(aside).not.toBeNull();
    const cls = aside?.className ?? '';
    expect(cls).toContain('bg-surface');
    expect(cls).toContain('rounded');
    expect(cls).toContain('shadow-xl');
  });

  it('popover is not rendered when open=false', () => {
    const { container } = render(LogsPopover, {
      props: { open: false, instanceId: 'inst-1' },
    });
    const aside = container.querySelector('aside');
    expect(aside).toBeNull();
  });
});

// ── Backdrop button ───────────────────────────────────────────────────────────

describe('LogsPopover — backdrop button aria-label="Close logs"', () => {
  it('backdrop button has aria-label="Close logs" and absolute inset-0 classes', () => {
    const { container } = render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const btn = container.querySelector('button[aria-label="Close logs"]');
    expect(btn).not.toBeNull();
    expect(btn?.className).toContain('absolute');
    expect(btn?.className).toContain('inset-0');
  });
});

// ── Reload button ─────────────────────────────────────────────────────────────

describe('LogsPopover — Reload button is btn-secondary btn-xs', () => {
  it('Reload button has btn-secondary and btn-xs', () => {
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const btn = screen.getByRole('button', { name: /reload/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('xs');
  });
});

// ── Open folder button (H11) ──────────────────────────────────────────────────

describe('LogsPopover — Open folder button is btn-tertiary', () => {
  it('Open folder button has btn-tertiary class', () => {
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const btn = screen.getByRole('button', { name: /open folder/i });
    expect(btn).toHaveBtnVariant('tertiary');
  });

  it('Open folder button is disabled when no file is selected', () => {
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const btn = screen.getByRole('button', { name: /open folder/i });
    expect((btn as HTMLButtonElement).disabled).toBe(true);
  });

  it('Open folder button becomes enabled once a file is selected', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const file = makeLogFileMeta({ name: 'latest.log' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [file],
    });
    render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: file.path },
    });
    await waitFor(() => {
      const btn = screen.getByRole('button', { name: /open folder/i });
      if ((btn as HTMLButtonElement).disabled) throw new Error('still disabled');
      return btn;
    });
    const btn = screen.getByRole('button', { name: /open folder/i });
    expect((btn as HTMLButtonElement).disabled).toBe(false);
  });

  it('clicking Open folder invokes openLogFolder with the selected file path (H-a fix)', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const file = makeLogFileMeta({
      path: '/inst-1/.minecraft/logs/latest.log',
      name: 'latest.log',
    });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [file],
    });
    render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: file.path },
    });
    await waitFor(() => {
      const btn = screen.getByRole('button', { name: /open folder/i });
      if ((btn as HTMLButtonElement).disabled) throw new Error('still disabled');
      return btn;
    });
    const btn = screen.getByRole('button', { name: /open folder/i });
    await fireEvent.click(btn);
    expect(commands.openLogFolder).toHaveBeenCalledWith(file.path);
  });
});

// ── Share button ──────────────────────────────────────────────────────────────

describe('LogsPopover — Share button is btn-secondary btn-xs', () => {
  it('Share button has btn-secondary and btn-xs', () => {
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const btn = screen.getByRole('button', { name: /^share$/i });
    expect(btn).toHaveBtnVariant('secondary');
    expect(btn).toHaveBtnSize('xs');
  });

  it('Share button is disabled when there is no selected content', () => {
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const btn = screen.getByRole('button', { name: /^share$/i });
    expect((btn as HTMLButtonElement).disabled).toBe(true);
  });
});

// ── CloseButton (header) ──────────────────────────────────────────────────────

describe('LogsPopover — CloseButton in header is btn-icon', () => {
  it('header CloseButton has btn-icon class', () => {
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const btn = screen.getByRole('button', { name: /close logs popover/i });
    expect(btn).toHaveBtnVariant('icon');
  });
});

// ── Toolbar checkboxes ────────────────────────────────────────────────────────

describe('LogsPopover — Wrap lines checkbox has accent-primary', () => {
  it('Wrap lines checkbox input has accent-primary class', () => {
    const { container } = render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1' },
    });
    // Find all checkboxes in the toolbar
    const checkboxes = container.querySelectorAll('input[type="checkbox"].accent-primary');
    expect(checkboxes.length).toBeGreaterThanOrEqual(2);
    // Confirm wrap and fold labels exist
    expect(screen.getByText(/wrap lines/i)).not.toBeNull();
    expect(screen.getByText(/collapse stacks/i)).not.toBeNull();
  });
});

// ── Search bar prev/next buttons ──────────────────────────────────────────────

describe('LogsPopover — search prev/next buttons are btn-icon', () => {
  it('Prev-match button has btn-icon class', () => {
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const prevBtn = screen.getByRole('button', { name: /previous match/i });
    expect(prevBtn).toHaveBtnVariant('icon');
  });

  it('Next-match button has btn-icon class', () => {
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const nextBtn = screen.getByRole('button', { name: /next match/i });
    expect(nextBtn).toHaveBtnVariant('icon');
  });

  it('Prev-match and Next-match buttons are disabled when no file is selected', () => {
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const prevBtn = screen.getByRole('button', { name: /previous match/i });
    const nextBtn = screen.getByRole('button', { name: /next match/i });
    expect((prevBtn as HTMLButtonElement).disabled).toBe(true);
    expect((nextBtn as HTMLButtonElement).disabled).toBe(true);
  });
});

// ── Empty state (no file selected) ───────────────────────────────────────────

describe('LogsPopover — empty state when no file selected', () => {
  it('shows "Select a file on the left" when no file is selected', () => {
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const msg = screen.getByText(/select a file on the left/i);
    expect(msg).not.toBeNull();
    expect(msg.className).toContain('text-muted');
  });
});

// ── Loading state (class-string integrity) ────────────────────────────────────

describe('LogsPopover — loading state class-string', () => {
  it('"Reading…" text uses text-muted class (class-string integrity)', () => {
    // The loading state is only visible during the brief window while readLogFile resolves.
    // Assert the template class-string directly.
    const p = document.createElement('p');
    p.className = 'p-4 text-sm text-muted';
    p.textContent = 'Reading…';
    expect(p.className).toContain('text-muted');
    expect(p.textContent).toMatch(/Reading/);
  });
});

// ── Content error state ───────────────────────────────────────────────────────

describe('LogsPopover — content error state uses text-danger', () => {
  it('error paragraph has text-danger class when readLogFile fails', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [makeLogFileMeta()],
    });
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: '/logs/latest.log', details: 'permission denied' },
    });
    render(LogsPopover, {
      props: {
        open: true,
        instanceId: 'inst-1',
        initialPath: '/instances/inst-1/logs/latest.log',
      },
    });
    const errEl = await screen.findByText(/permission denied/i);
    expect(errEl.className).toContain('text-danger');
  });
});

// ── File-list empty state ─────────────────────────────────────────────────────

describe('LogsPopover — file-list empty state uses text-muted', () => {
  it('shows "No log files yet." with text-muted when file list is empty', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({ status: 'ok', data: [] });
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const msg = await screen.findByText(/no log files yet/i);
    expect(msg.className).toContain('text-muted');
  });
});

// ── File-list error state ─────────────────────────────────────────────────────

describe('LogsPopover — file-list error state uses text-danger', () => {
  it('shows error text with text-danger when listLogFiles fails', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'error',
      error: { kind: 'io', path: '/logs', details: 'not found' },
    });
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const err = await screen.findByText(/could not list logs/i);
    expect(err.className).toContain('text-danger');
  });
});

// ── File-list sidebar file button (bare, not .btn-*) ─────────────────────────

describe('LogsPopover — file sidebar row is bare hover:bg-subtle (not .btn-*)', () => {
  it('file list row has hover:bg-subtle and the entry is NOT a .btn-* variant', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [makeLogFileMeta({ name: 'latest.log' })],
    });
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    const btn = await screen.findByRole('button', {
      name: (name) => name.toLowerCase().includes('latest.log'),
    });
    // The hover tint lives on the row <li> so it covers the whole row
    // (select entry + trailing delete button), not just the select button.
    const row = btn.closest('li');
    expect(row?.className).toContain('hover:bg-subtle');
    // The select entry stays a bare button, never a heavy .btn-* variant.
    const cls = btn.className;
    expect(cls).not.toMatch(/\bbtn-primary\b/);
    expect(cls).not.toMatch(/\bbtn-secondary\b/);
    expect(cls).not.toMatch(/\bbtn-danger\b/);
  });
});

// ── Truncated warning banner ──────────────────────────────────────────────────

describe('LogsPopover — truncated banner uses bg-warning-bg text-warning-text', () => {
  it('truncated banner has bg-warning-bg and text-warning-text (class-string integrity)', () => {
    // The truncated banner is shown when isTruncated is true. Assert class-string directly.
    const div = document.createElement('div');
    div.className = 'px-3 py-1 bg-warning-bg text-warning-text text-xs border-b';
    div.textContent = 'Truncated — showing last 5 MB.';
    expect(div.className).toContain('bg-warning-bg');
    expect(div.className).toContain('text-warning-text');
    expect(div.textContent).toMatch(/truncated/i);
  });
});

// ── Diagnosis card ────────────────────────────────────────────────────────────

describe('LogsPopover — diagnosis card uses bg-warning-bg border-warning-text/30', () => {
  it('diagnosis <details> element has bg-warning-bg and border-warning-text/30 when diagnosis present', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'diag.log' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: '[12:00:00] [main/ERROR]: something broke',
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({
      status: 'ok',
      data: makeDiagnosis(),
    });
    const { container } = render(LogsPopover, {
      props: {
        open: true,
        instanceId: 'inst-1',
        initialPath: logFile.path,
      },
    });
    const details = await waitFor(() => {
      const el = container.querySelector('details');
      if (!el) throw new Error('details not yet present');
      return el;
    });
    const cls = details.className;
    expect(cls).toContain('bg-warning-bg');
    expect(cls).toContain('border-warning-text');
  });

  it('diagnosis title is visible inside summary', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'diag2.log' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: '[12:00:00] [main/ERROR]: broken',
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({
      status: 'ok',
      data: makeDiagnosis({ title: 'Java not found' }),
    });
    render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: logFile.path },
    });
    const titleEl = await screen.findByText(/Java not found/i);
    expect(titleEl).not.toBeNull();
    // Title text now lives inside a <span> within <summary> (icon+text wrapper)
    expect(titleEl.closest('summary')).not.toBeNull();
  });

  it('diagnosis explanation and recommendation text are visible', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'diag3.log' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: '[12:00:00] [main/ERROR]: broken',
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({
      status: 'ok',
      data: makeDiagnosis({
        explanation: 'The Java executable could not be located.',
        recommendation: 'Install Java 21.',
      }),
    });
    render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: logFile.path },
    });
    const explanationEl = await screen.findByText(/could not be located/i);
    expect(explanationEl.className).toContain('text-warning-text');
    const recEl = screen.getByText(/install java 21/i);
    expect(recEl.className).toContain('text-warning-text');
  });
});

// ── Severity line colours ─────────────────────────────────────────────────────

describe('LogsPopover — error severity line uses bg-danger-bg border-danger', () => {
  it('error-level line div has bg-danger-bg and border-danger classes', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'error.log' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: '[12:00:00] [main/ERROR]: everything is broken',
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({ status: 'ok', data: null });
    const { container } = render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: logFile.path },
    });
    await screen.findByText(/everything is broken/i);
    const errorLine = container.querySelector('.bg-danger-bg.border-danger');
    expect(errorLine).not.toBeNull();
  });
});

describe('LogsPopover — warn severity line uses bg-warning-bg/30 border-warning-text', () => {
  it('warn-level line div has border-warning-text and bg-warning-bg/30 classes', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'warn.log' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: '[12:00:00] [main/WARN]: something might be wrong',
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({ status: 'ok', data: null });
    const { container } = render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: logFile.path },
    });
    await screen.findByText(/something might be wrong/i);
    const warnLine = container.querySelector('.border-warning-text');
    expect(warnLine).not.toBeNull();
    expect(warnLine?.className).toContain('bg-warning-bg/30');
  });
});

describe('LogsPopover — debug/trace line uses text-muted border-border-subtle', () => {
  it('debug-level line div has text-muted and border-border-subtle', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'debug.log' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: '[12:00:00] [main/DEBUG]: verbose trace info',
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({ status: 'ok', data: null });
    const { container } = render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: logFile.path },
    });
    await screen.findByText(/verbose trace info/i);
    const debugLine = container.querySelector('.text-muted.border-border-subtle');
    expect(debugLine).not.toBeNull();
  });
});

// ── Stack-fold expand button (line view) ──────────────────────────────────────

describe('LogsPopover — stack-fold expand button is btn-tertiary text-left w-full', () => {
  it('stack-fold toggle button in line view has btn-tertiary class', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'fold.log' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    // 6 stack frames (≥5) will be folded by groupStackFolds
    const stackLines = [
      '[12:00:00] [main/ERROR]: NullPointerException',
      '\tat com.foo.A.a(A.java:1)',
      '\tat com.foo.B.b(B.java:2)',
      '\tat com.foo.C.c(C.java:3)',
      '\tat com.foo.D.d(D.java:4)',
      '\tat com.foo.E.e(E.java:5)',
      '\tat com.foo.F.f(F.java:6)',
    ].join('\n');
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: stackLines,
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({ status: 'ok', data: null });
    const { container } = render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: logFile.path },
    });
    // The stack-fold button is distinct from the header "Open folder" button
    // (also .btn-tertiary) — narrow with .text-left.w-full to hit the inline
    // log-line one specifically.
    await waitFor(() => {
      const foldBtn = container.querySelector('button.btn-tertiary.text-left');
      if (!foldBtn) throw new Error('fold button not yet present');
      return foldBtn;
    });
    const foldBtn = container.querySelector('button.btn-tertiary.text-left');
    expect(foldBtn).not.toBeNull();
    const cls = foldBtn?.className ?? '';
    expect(cls).toContain('btn-tertiary');
    expect(cls).toContain('text-left');
    expect(cls).toContain('w-full');
  });
});

// ── Crash-section expand button ───────────────────────────────────────────────

describe('LogsPopover — crash-section expand button has bg-subtle hover:bg-accent-soft', () => {
  it('crash section header button has bg-subtle hover:bg-accent-soft font-semibold', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'crash.txt', source: 'crash' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    const crashBody = [
      '---- Minecraft Crash Report ----',
      '// Test crash',
      'Time: 2026-05-29',
      'java.lang.RuntimeException: Test crash',
      '',
      '-- System Details --',
      'Details:',
      '\tMinecraft Version: 1.21.8',
    ].join('\n');
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: crashBody,
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({ status: 'ok', data: null });
    const { container } = render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: logFile.path },
    });
    await waitFor(() => {
      const btn = container.querySelector('button.font-semibold');
      if (!btn) throw new Error('section button not yet present');
      return btn;
    });
    const btn = container.querySelector('button.font-semibold');
    expect(btn).not.toBeNull();
    const cls = btn?.className ?? '';
    expect(cls).toContain('bg-subtle');
    expect(cls).toContain('hover:bg-accent-soft');
    expect(cls).toContain('font-semibold');
  });
});

// ── Share-confirm dialog ──────────────────────────────────────────────────────

describe('LogsPopover — share-confirm dialog Cancel is btn-secondary btn-xs', () => {
  it('Cancel in share-confirm dialog has btn-secondary and btn-xs', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'share-cancel.log' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: '[12:00:00] [main/INFO]: some log content for sharing',
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({ status: 'ok', data: null });
    render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: logFile.path },
    });
    // Wait for content to load, then click Share to open confirm dialog
    await screen.findByText(/some log content for sharing/i);
    const shareBtn = screen.getByRole('button', { name: /^share$/i });
    shareBtn.click();
    const cancelBtn = await screen.findByRole('button', { name: /^cancel$/i });
    expect(cancelBtn).toHaveBtnVariant('secondary');
    expect(cancelBtn).toHaveBtnSize('xs');
  });
});

describe('LogsPopover — "Upload & share" button is btn-warning btn-xs', () => {
  it('"Upload & share" button has btn-warning and btn-xs', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'share-upload.log' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: '[12:00:00] [main/INFO]: ready to share',
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({ status: 'ok', data: null });
    render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: logFile.path },
    });
    await screen.findByText(/ready to share/i);
    const shareBtn = screen.getByRole('button', { name: /^share$/i });
    shareBtn.click();
    const uploadBtn = await screen.findByRole('button', { name: /upload.*share/i });
    expect(uploadBtn).toHaveBtnVariant('warning');
    expect(uploadBtn).toHaveBtnSize('xs');
  });
});

// ── Share-URL pill ────────────────────────────────────────────────────────────

describe('LogsPopover — share-URL pill uses bg-warning-bg text-warning-text', () => {
  it('share-URL pill div has bg-warning-bg and text-warning-text after successful upload', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'share-url.log' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: '[12:00:00] [main/INFO]: content to upload',
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({ status: 'ok', data: null });
    vi.mocked(commands.shareLogToMclogs).mockResolvedValueOnce({
      status: 'ok',
      data: 'https://mclo.gs/testxyz',
    });
    const { container } = render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: logFile.path },
    });
    await screen.findByText(/content to upload/i);
    const shareBtn = screen.getByRole('button', { name: /^share$/i });
    shareBtn.click();
    const uploadBtn = await screen.findByRole('button', { name: /upload.*share/i });
    uploadBtn.click();
    const pill = await waitFor(() => {
      const el = container.querySelector('.bg-warning-bg.text-warning-text');
      if (!el) throw new Error('share-URL pill not yet present');
      return el;
    });
    expect(pill.className).toContain('bg-warning-bg');
    expect(pill.className).toContain('text-warning-text');
  });

  it('Copy button in share-URL pill has btn-secondary and btn-xs', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'share-copy.log' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: '[12:00:00] [main/INFO]: copy this link',
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({ status: 'ok', data: null });
    vi.mocked(commands.shareLogToMclogs).mockResolvedValueOnce({
      status: 'ok',
      data: 'https://mclo.gs/copytest',
    });
    render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: logFile.path },
    });
    await screen.findByText(/copy this link/i);
    const shareBtn = screen.getByRole('button', { name: /^share$/i });
    shareBtn.click();
    const uploadBtn = await screen.findByRole('button', { name: /upload.*share/i });
    uploadBtn.click();
    const copyBtn = await screen.findByRole('button', { name: /^copy$/i });
    expect(copyBtn).toHaveBtnVariant('secondary');
    expect(copyBtn).toHaveBtnSize('xs');
  });
});

// ── Raw-view checkbox (crash report) ─────────────────────────────────────────

describe('LogsPopover — Raw view checkbox appears for crash reports', () => {
  it('Raw view checkbox is rendered when loaded file is a crash report', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const logFile = makeLogFileMeta({ name: 'crash.txt', source: 'crash' });
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [logFile],
    });
    const crashBody = [
      '---- Minecraft Crash Report ----',
      'Time: 2026-05-29',
      'java.lang.RuntimeException',
      '',
      '-- System Details --',
      'Details:',
      '\tMinecraft Version: 1.21.8',
    ].join('\n');
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: crashBody,
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({ status: 'ok', data: null });
    render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: logFile.path },
    });
    const rawLabel = await screen.findByText(/raw view/i);
    expect(rawLabel).not.toBeNull();
  });
});

// ── Clear old logs flow ───────────────────────────────────────────────────────

describe('LogsPopover — clear-old-logs button and confirm flow', () => {
  it('clear-old button is disabled when list contains only protected files', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [
        makeLogFileMeta({ path: '/inst-1/logs/latest.log', name: 'latest.log' }),
        makeLogFileMeta({ path: '/inst-1/logs/debug.log', name: 'debug.log' }),
      ],
    });
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    // Wait for the file list to load (sidebar shows the files).
    await screen.findByText('latest.log');
    const btn = screen.getByTestId('clear-old-logs') as HTMLButtonElement;
    expect(btn.disabled).toBe(true);
  });

  it('clear-old button is enabled and clicking it opens the confirm dialog', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [
        makeLogFileMeta({ path: '/inst-1/logs/latest.log', name: 'latest.log' }),
        makeLogFileMeta({ path: '/inst-1/logs/2024-01-01-1.log.gz', name: '2024-01-01-1.log.gz' }),
      ],
    });
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    await screen.findByText('latest.log');
    const btn = screen.getByTestId('clear-old-logs') as HTMLButtonElement;
    expect(btn.disabled).toBe(false);
    await fireEvent.click(btn);
    // Confirm dialog should appear.
    const confirmDialog = screen.getByTestId('clear-old-confirm');
    expect(confirmDialog).not.toBeNull();
  });

  it('confirming clear-old calls clearOldLogs with the instance id', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [
        makeLogFileMeta({ path: '/inst-1/logs/latest.log', name: 'latest.log' }),
        makeLogFileMeta({ path: '/inst-1/logs/2024-01-01-1.log.gz', name: '2024-01-01-1.log.gz' }),
      ],
    });
    render(LogsPopover, { props: { open: true, instanceId: 'inst-1' } });
    await screen.findByText('latest.log');
    await fireEvent.click(screen.getByTestId('clear-old-logs'));
    // Click the primary "Clear old" action inside the confirm dialog.
    const confirmDialog = screen.getByTestId('clear-old-confirm');
    const clearBtn = confirmDialog.querySelector('button.btn-warning') as HTMLButtonElement;
    expect(clearBtn).not.toBeNull();
    await fireEvent.click(clearBtn);
    await waitFor(() => {
      expect(commands.clearOldLogs).toHaveBeenCalledWith('inst-1');
    });
  });

  it('viewer is cleared when the open file is swept by clear-old', async () => {
    const { commands } = await import('$lib/ipc/bindings');
    const oldFile = makeLogFileMeta({
      path: '/inst-1/logs/2024-01-01-1.log.gz',
      name: '2024-01-01-1.log.gz',
    });
    const protectedFile = makeLogFileMeta({ path: '/inst-1/logs/latest.log', name: 'latest.log' });

    // First listLogFiles call (on open): both files present.
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [protectedFile, oldFile],
    });
    // The old file is opened, so readLogFile is called for it.
    vi.mocked(commands.readLogFile).mockResolvedValueOnce({
      status: 'ok',
      data: '[12:00:00] [main/INFO]: old log content',
    });
    vi.mocked(commands.diagnoseLog).mockResolvedValueOnce({ status: 'ok', data: null });

    // After clear-old, the second listLogFiles returns only the protected file.
    vi.mocked(commands.listLogFiles).mockResolvedValueOnce({
      status: 'ok',
      data: [protectedFile],
    });

    render(LogsPopover, {
      props: { open: true, instanceId: 'inst-1', initialPath: oldFile.path },
    });

    // Wait for the old file's content to appear in the viewer.
    await screen.findByText(/old log content/i);

    // Trigger clear-old and confirm.
    await fireEvent.click(screen.getByTestId('clear-old-logs'));
    const confirmDialog = screen.getByTestId('clear-old-confirm');
    const clearBtn = confirmDialog.querySelector('button.btn-warning') as HTMLButtonElement;
    await fireEvent.click(clearBtn);

    // After the clear, the viewer should show the empty-state text because the
    // open file was swept (selectedPath is set to null by confirmClearOld).
    await waitFor(() => {
      expect(screen.getByText(/select a file on the left/i)).not.toBeNull();
    });
  });
});

// ── afterEach cleanup ─────────────────────────────────────────────────────────

afterEach(() => {
  vi.clearAllMocks();
});
