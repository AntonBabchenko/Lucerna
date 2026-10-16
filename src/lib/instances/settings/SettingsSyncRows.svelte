<script lang="ts">
  // Shared game settings (spec 2026-10-15 §B9): one checkbox per kind this
  // instance can share with the others, each with what it is doing now. The
  // backend owns every decision (§B7); this asks only the two questions a
  // join can raise: which hotbars to keep, and whether a game Lucerna lost
  // track of is closed.
  import { commands } from '$lib/ipc/bindings';
  import type { Join, Kind, KindView, SectionState } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import ConfirmDialog from '$lib/ui/ConfirmDialog.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import LoadingPanel from '$lib/ui/LoadingPanel.svelte';
  import {
    failureDetail,
    failureWhat,
    joinChoices,
    KIND_LABEL,
    REASON,
    rowLine,
  } from './settings-sync-view';

  let {
    instanceId,
    running,
    busy,
  }: {
    instanceId: string;
    /** THIS instance's game is up: what it shares cannot change under it. */
    running: boolean;
    /** A task holds this instance. */
    busy: boolean;
  } = $props();

  type Loaded =
    | { kind: 'loading' }
    | { kind: 'ok'; data: SectionState }
    | { kind: 'failed'; error: string };
  let loaded = $state<Loaded>({ kind: 'loading' });
  /** The kind whose change is in flight. */
  let changing = $state<Kind | null>(null);
  let changeError = $state<string | null>(null);
  let joinAsk = $state<{ choices: Exclude<Join, 'done'>[] } | null>(null);
  let closedAsk = $state<{ retry: () => Promise<void> } | null>(null);
  // The checkbox the user just clicked. `checked` follows the backend, which
  // a cancelled question or a failed change leaves as it was, and Svelte only
  // writes the DOM on a CHANGE: so the click is undone here, by hand.
  let clicked: { el: HTMLInputElement; was: boolean } | null = null;
  function undoClick() {
    if (clicked) clicked.el.checked = clicked.was;
    clicked = null;
  }

  const locked = $derived(running || busy);
  const data = $derived(loaded.kind === 'ok' ? loaded.data : null);
  const unreadable = $derived(data?.sidecar.status === 'unreadable' ? data.sidecar.reason : null);

  // A later load wins: switching instances mid-read must not show the old one.
  let seq = 0;
  // Whose state is on screen. Another instance's rows are never shown, not
  // even while this one's are read: they would act on this one.
  let shownFor: string | null = null;
  async function load(id: string) {
    const mine = ++seq;
    if (shownFor !== id) loaded = { kind: 'loading' };
    try {
      const r = await commands.settingsSyncState(id);
      if (mine !== seq) return;
      shownFor = id;
      loaded =
        r.status === 'ok'
          ? { kind: 'ok', data: r.data }
          : { kind: 'failed', error: formatError(r.error) };
    } catch (e) {
      if (mine !== seq) return;
      loaded = { kind: 'failed', error: e instanceof Error ? e.message : String(e) };
    }
  }

  // Re-read when the instance changes and when its game stops: the exit step
  // may have kept a failure to show.
  $effect(() => {
    const id = instanceId;
    void running;
    changeError = null;
    void load(id);
  });

  async function apply(
    kind: Kind,
    enabled: boolean,
    join: Exclude<Join, 'done'> | null,
    confirmed = false,
  ) {
    const id = instanceId;
    changing = kind;
    changeError = null;
    try {
      const r = await commands.settingsSyncSetKind(id, kind, enabled, join, confirmed);
      if (r.status === 'error') {
        if (r.error.kind === 'settings_sync_ask_game_closed' && !confirmed) {
          closedAsk = { retry: () => apply(kind, enabled, join, true) };
          return;
        }
        undoClick();
        changeError = $t('instance.settingsSync.changeFailed', { error: formatError(r.error) });
      } else {
        clicked = null;
      }
    } catch (e) {
      undoClick();
      changeError = $t('instance.settingsSync.changeFailed', {
        error: e instanceof Error ? e.message : String(e),
      });
    } finally {
      changing = null;
    }
    await load(id);
  }

  function onToggle(el: HTMLInputElement, v: KindView) {
    const on = el.checked;
    clicked = { el, was: v.enabled };
    const choices = on ? joinChoices(v) : null;
    if (choices) {
      joinAsk = { choices };
      return;
    }
    void apply(v.kind, on, null);
  }

  let stopping = $state(false);
  async function stopAll() {
    if (stopping) return;
    const id = instanceId;
    stopping = true;
    changeError = null;
    try {
      const r = await commands.settingsSyncStop(id, null);
      if (r.status === 'error') {
        changeError = $t('instance.settingsSync.changeFailed', { error: formatError(r.error) });
      }
    } catch (e) {
      changeError = $t('instance.settingsSync.changeFailed', {
        error: e instanceof Error ? e.message : String(e),
      });
    } finally {
      stopping = false;
    }
    await load(id);
  }

  /** The ids a checkbox is described by: its state line, and why it is locked. */
  function describedBy(v: KindView, line: string | null): string | undefined {
    const ids = [
      line ? `settings-sync-state-${v.kind}` : null,
      locked ? 'settings-sync-locked' : null,
    ];
    return ids.filter(Boolean).join(' ') || undefined;
  }
</script>

<div class="mt-4 flex flex-col" data-testid="settings-sync">
  <h4 class="mb-1 text-sm font-medium text-primary">{$t('instance.settingsSync.title')}</h4>
  <p class="mb-2 text-xs text-secondary">{$t('instance.settingsSync.hint')}</p>

  {#if loaded.kind === 'loading'}
    <LoadingPanel label={$t('common.loading')} size="sm" />
  {:else if loaded.kind === 'failed'}
    <div class="flex flex-col gap-1">
      <StatusMessage
        dataTestid="settings-sync-load-failed"
        message={$t('instance.settingsSync.loadFailed', { error: loaded.error })}
        tone="danger"
      />
      <div>
        <button
          type="button"
          class="btn-secondary btn-sm"
          onclick={() => void load(instanceId)}
          data-testid="settings-sync-retry"
        >
          {$t('instance.settingsSync.retry')}
        </button>
      </div>
    </div>
  {:else if data}
    {#if unreadable !== null}
      <div class="flex flex-col gap-1" data-testid="settings-sync-unreadable">
        <StatusMessage
          message={$t('instance.settingsSync.unreadable', { reason: $t(REASON[unreadable]) })}
          tone="warning"
        />
        <p class="text-xs text-muted">{$t('instance.settingsSync.unreadableHint')}</p>
        <div>
          <button
            type="button"
            class="btn-secondary btn-sm"
            disabled={locked || stopping}
            onclick={() => void stopAll()}
            data-testid="settings-sync-stop-all"
          >
            {$t('instance.settingsSync.unreadableFix')}
          </button>
        </div>
      </div>
    {:else}
      <ul class="flex flex-col gap-2">
        {#each data.kinds as v (v.kind)}
          {@const line = rowLine($t, v)}
          <li data-testid="settings-sync-row-{v.kind}">
            <label class="flex items-start gap-2 cursor-pointer">
              <input
                type="checkbox"
                class="mt-0.5"
                checked={v.enabled}
                disabled={locked || changing !== null}
                aria-describedby={describedBy(v, line)}
                onchange={(e) => onToggle(e.currentTarget, v)}
                data-testid="settings-sync-toggle-{v.kind}"
              />
              <span class="text-sm text-primary">{$t(KIND_LABEL[v.kind])}</span>
            </label>
            {#if line}
              <p
                id="settings-sync-state-{v.kind}"
                class="pl-6 text-xs text-muted"
                data-testid="settings-sync-state-{v.kind}"
              >
                {line}
              </p>
            {/if}
            {#if v.kind === 'hotbars' && !v.enabled}
              <p class="pl-6 text-xs text-muted">{$t('instance.settingsSync.hotbarsRule')}</p>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
    {#if data.last_error}
      <StatusMessage
        class="mt-2"
        dataTestid="settings-sync-last-error"
        message={$t('instance.settingsSync.lastError', {
          what: $t(failureWhat(data.last_error)),
          detail: failureDetail($t, data.last_error),
        })}
        tone="warning"
      />
    {/if}
    {#if locked}
      <p class="mt-1 text-xs text-muted" data-testid="settings-sync-locked">
        {running
          ? $t('instance.settingsSync.lockedRunning')
          : $t('instance.settingsSync.lockedBusy')}
      </p>
    {/if}
    <StatusMessage
      class="mt-2"
      dataTestid="settings-sync-change-failed"
      message={changeError}
      tone="danger"
    />
  {/if}
</div>

{#if joinAsk}
  {@const choices = joinAsk?.choices ?? []}
  {@const both = choices.includes('use_shared')}
  <!-- Joining means taking the shared hotbars: that is the headline. Making
       this instance's the shared ones overwrites every other instance's, so it
       is the alternative, and the only choice for an older instance. -->
  <ConfirmDialog
    title={$t('instance.settingsSync.joinTitle')}
    bodyText={[
      both ? $t('instance.settingsSync.joinBody') : $t('instance.settingsSync.joinOlderBody'),
      $t('instance.settingsSync.joinKept'),
      $t('instance.settingsSync.hotbarsRule'),
    ]}
    confirmLabel={both
      ? $t('instance.settingsSync.useShared')
      : $t('instance.settingsSync.makeShared')}
    confirmTestid={both ? 'settings-sync-use-shared' : 'settings-sync-make-shared'}
    secondaryLabel={both ? $t('instance.settingsSync.makeShared') : undefined}
    secondaryTestid="settings-sync-make-shared"
    onSecondary={both
      ? () => {
          joinAsk = null;
          void apply('hotbars', true, 'make_shared');
        }
      : undefined}
    onCancel={() => {
      joinAsk = null;
      undoClick();
    }}
    onConfirm={() => {
      // Read before clearing: `both` derives from `joinAsk` and reads false after.
      const join = both ? 'use_shared' : 'make_shared';
      joinAsk = null;
      void apply('hotbars', true, join);
    }}
  />
{/if}

{#if closedAsk}
  <ConfirmDialog
    title={$t('launch.settingsSync.closedTitle')}
    bodyText={$t('launch.settingsSync.closedBody')}
    confirmLabel={$t('launch.settingsSync.itsClosed')}
    confirmTestid="settings-sync-its-closed"
    onCancel={() => {
      closedAsk = null;
      undoClick();
    }}
    onConfirm={() => {
      // Taken before clearing: anything derived from `closedAsk` reads null after.
      const ask = closedAsk;
      closedAsk = null;
      void ask?.retry();
    }}
  />
{/if}
