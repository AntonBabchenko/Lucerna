import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import DataLocationProgressDialog from '$lib/settings/DataLocationProgressDialog.svelte';
import type { RelocationView } from '$lib/settings/data-location.svelte';

const running = (phase: string | null, progress: unknown = null) =>
  ({ kind: 'running', phase, progress }) as RelocationView;
// The full generated `restart_required` shape. `old_root_is_default` and `retry_possible` are the
// BACKEND's verdicts (a canonical path compare; "some leftover is not launcher-owned") — the dialog
// reads them and derives neither from paths nor from the list.
const final = (over: Record<string, unknown> = {}) =>
  ({
    kind: 'restart_required',
    details: {
      kind: 'restart_required',
      old_root: 'C:\\Old',
      new_root: 'D:\\New',
      leftovers: ['logs', 'webview'],
      old_root_intact: false,
      old_root_is_default: false,
      retry_possible: true,
      ...over,
    },
  }) as RelocationView;

function mount(view: RelocationView, over: Record<string, unknown> = {}) {
  const handlers = {
    onCancel: vi.fn(),
    onRetry: vi.fn(),
    onOpenFolder: vi.fn(),
    onRestart: vi.fn(),
  };
  const utils = render(DataLocationProgressDialog, { props: { view, ...handlers, ...over } });
  return { ...utils, ...handlers };
}
const button = (name: string) => screen.queryByRole('button', { name }) as HTMLButtonElement | null;
const phaseText = () => screen.getByTestId('data-move-phase').textContent;

describe('DataLocationProgressDialog — running', () => {
  it('exposes the copy as a progressbar and keeps the percentage out of the live region', () => {
    mount(running('copying', { copied_bytes: 50, total_bytes: 200, phase: 'copying' }));
    expect(phaseText()).toBe('Copying files…');
    const bar = screen.getByRole('progressbar', { name: 'Copy progress' });
    expect(bar.getAttribute('aria-valuenow')).toBe('25');
    expect(bar.closest('[role="status"]')).toBeNull();
    expect(screen.getByText('25%').closest('[role="status"]')).toBeNull();
  });

  it.each([
    'copying',
    'verifying',
  ])('offers Cancel while %s, as a small secondary button', (phase) => {
    mount(running(phase));
    expect(button('Cancel move')).toHaveBtnVariant('secondary');
    expect(button('Cancel move')).toHaveBtnSize('sm');
  });

  it.each([null, 'switching', 'deleting'])('offers no Cancel and no bar at phase %s', (phase) => {
    mount(running(phase));
    expect(button('Cancel move')).toBeNull();
    expect(screen.queryByRole('progressbar')).toBeNull();
  });

  it('says it is too late to cancel once the switch has started', () => {
    mount(running('switching'), { cancelling: true });
    expect(phaseText()).toContain('too late to cancel');
    expect(screen.getByText(/had already finished/)).toBeTruthy();
  });

  it('keeps its label and shows busy while a cancel is pending', () => {
    mount(running('copying'), { cancelling: true });
    // By test id: a busy BusyButton holds its Spinner (role="status", "Loading…"), which joins the
    // button's accessible name — an exact-name role query no longer finds it.
    const cancel = screen.getByTestId('data-move-cancel') as HTMLButtonElement;
    expect(cancel.textContent).toContain('Cancel move');
    expect(cancel.disabled).toBe(true);
    expect(cancel.getAttribute('aria-busy')).toBe('true');
  });

  it('parks focus on the dialog body before the pressed button can turn disabled', async () => {
    const { onCancel } = mount(running('verifying'));
    const cancel = button('Cancel move') as HTMLButtonElement;
    cancel.focus();
    await fireEvent.click(cancel);
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(screen.getByTestId('data-move-body'));
  });
});

describe('DataLocationProgressDialog — final state', () => {
  it('names both folders and every leftover, offers the three actions, and focuses Restart', async () => {
    const { onRetry, onOpenFolder } = mount(final());
    expect(screen.getByText('Data moved — restart to finish')).toBeTruthy();
    expect(screen.getByText('Your data was moved to "D:\\New".')).toBeTruthy();
    expect(
      screen.getByText('Lucerna could not remove 2 items from the old folder "C:\\Old":'),
    ).toBeTruthy();
    const items = screen.getByTestId('data-move-leftovers').querySelectorAll('li');
    expect([...items].map((li) => li.textContent)).toEqual(['logs', 'webview']);
    expect(button('Restart')).toHaveBtnVariant('primary');
    expect(button('Try again')).toHaveBtnVariant('secondary');
    expect(button('Open old folder')).toHaveBtnVariant('secondary');
    expect(screen.queryByTestId('data-move-keep-redirect')).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(button('Restart')));
    await fireEvent.click(button('Try again') as HTMLButtonElement);
    await fireEvent.click(button('Open old folder') as HTMLButtonElement);
    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(onOpenFolder).toHaveBeenCalledTimes(1);
  });

  it('moves focus to Restart when a running dialog turns into the final state', async () => {
    const { rerender } = mount(running('deleting'));
    await rerender({ view: final() });
    await waitFor(() => expect(document.activeElement).toBe(button('Restart')));
  });

  // The rule is the backend's `retry_possible` and nothing else: the same listed leftovers, the
  // same not-intact old folder — only the flag differs.
  it.each([
    [true, true],
    [false, false],
  ])('offers Try again if and only if the backend says a retry can help (retry_possible=%s)', (retryPossible, offered) => {
    mount(
      final({
        leftovers: ['logs', 'webview'],
        old_root_intact: false,
        retry_possible: retryPossible,
      }),
    );
    expect(button('Try again') !== null).toBe(offered);
    // Whatever the flag says, the leftovers stay listed and the way forward stays offered.
    expect(screen.getByTestId('data-move-leftovers')).toBeTruthy();
    expect(button('Restart')).not.toBeNull();
  });

  it('says the launcher removes its own leftovers only when nothing else is left', () => {
    // Only launcher-owned names remain: a retry cannot help, so the dialog says who will.
    const owned = mount(final({ leftovers: ['webview'], retry_possible: false }));
    expect(screen.getByTestId('data-move-own-leftovers').textContent).toContain(
      'Lucerna removes it on its own after the restart',
    );
    owned.unmount();
    // Something the user (or a retry) can still remove is listed: the promise would be false.
    const mixed = mount(final({ leftovers: ['logs', 'webview'], retry_possible: true }));
    expect(screen.queryByTestId('data-move-own-leftovers')).toBeNull();
    mixed.unmount();
    // Nothing was deleted: the list is not "leftovers" at all.
    mount(final({ old_root_intact: true, retry_possible: false }));
    expect(screen.queryByTestId('data-move-own-leftovers')).toBeNull();
  });

  it('offers no retry when nothing was deleted, and says the old copy is complete', () => {
    mount(final({ old_root_intact: true, leftovers: [], retry_possible: false }));
    expect(button('Try again')).toBeNull();
    expect(screen.getByText(/still complete\. You can delete that whole folder/)).toBeTruthy();
  });

  it('protects the redirect file when the old folder is the default folder', () => {
    mount(final({ old_root_is_default: true }));
    expect(screen.getByTestId('data-move-keep-redirect').textContent).toContain(
      'Keep data-location.json there',
    );
  });

  it('never says "delete the whole folder" about the default folder', () => {
    mount(final({ old_root_intact: true, old_root_is_default: true, retry_possible: false }));
    expect(screen.getByText(/except data-location\.json/)).toBeTruthy();
    expect(screen.queryByText(/delete that whole folder/)).toBeNull();
  });

  it('reports a successful retry and withdraws the retry button', () => {
    mount(final({ leftovers: [], retry_possible: false }));
    expect(screen.getByText('The leftovers in "C:\\Old" have been removed.')).toBeTruthy();
    expect(button('Try again')).toBeNull();
  });

  it('still offers Restart — and only Restart — when the details could not be loaded', () => {
    mount({ kind: 'restart_required', details: null });
    expect(screen.getByText(/couldn't load the details/)).toBeTruthy();
    expect(button('Restart')).not.toBeNull();
    expect(button('Open old folder')).toBeNull();
    expect(button('Try again')).toBeNull();
  });

  it('shows a failed restart, as an alert, with the way out', () => {
    mount(final(), { restartError: "Couldn't restart: boom Close Lucerna and open it again." });
    // Rendered after mount, so it must be an alert, not a quiet paragraph.
    expect(
      screen.getByText(/Close Lucerna and open it again/).closest('[role="alert"]'),
    ).not.toBeNull();
  });

  it('keeps the other actions inert — but focusable — while a retry runs', async () => {
    const { onOpenFolder } = mount(final(), { retrying: true });
    const open = button('Open old folder') as HTMLButtonElement;
    expect(open.getAttribute('aria-disabled')).toBe('true');
    expect(open.disabled).toBe(false);
    await fireEvent.click(open);
    expect(onOpenFolder).not.toHaveBeenCalled();
    expect(button('Restart')?.disabled).toBe(true);
  });
});
