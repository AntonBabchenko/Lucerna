<script lang="ts">
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/ipc/format-error';
  import type { ClientModFinding } from '$lib/ipc/bindings';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { diagnosisDismiss } from '$lib/ui/diagnosis-dismiss.svelte';
  import { serverDiagnosisSignature } from './server-diagnosis-view';

  let { serverId }: { serverId: string } = $props();

  const diag = $derived(serverState.diagnosisFor(serverId));
  // A running server hasn't crashed — never show a crash/repair banner while it
  // is up, even if a stale or non-fatal-warning diagnosis lingers in the store.
  const running = $derived(serverState.running(serverId));

  // The user can dismiss the banner ("I know about this problem"). Dismissal is
  // keyed by the diagnosis signature, so a different/new crash resurfaces it on
  // its own; the restore badge in ServerManageView brings the same one back.
  const signature = $derived(serverDiagnosisSignature(diag));
  const dismissed = $derived(
    signature !== null && diagnosisDismiss.isDismissed(`server:${serverId}`, signature),
  );

  // Map pattern_id → i18n subkey. Unknown patterns fall through to raw title.
  type PatternKey =
    | 'clientOnly'
    | 'portInUse'
    | 'eula'
    | 'orphan'
    | 'oom'
    | 'heapTooBig'
    | 'javaTooOld'
    | 'corruptJar'
    | 'modConflict'
    | 'missingDep'
    | 'fabricApiMissing'
    | 'mixinCrash'
    | 'worldCorrupt'
    | 'sessionLock'
    | 'wrongLoader'
    | 'diskLow'
    | 'crashUnknown';

  function patternKey(patternId: string): PatternKey | null {
    switch (patternId) {
      case 'server-client-only-mod-crash':
        return 'clientOnly';
      case 'server-port-in-use':
        return 'portInUse';
      case 'server-eula-not-accepted':
        return 'eula';
      case 'server-orphan-running':
        return 'orphan';
      case 'server-out-of-memory':
        return 'oom';
      case 'server-heap-too-big':
        return 'heapTooBig';
      case 'server-java-too-old':
        return 'javaTooOld';
      case 'server-corrupt-jar':
        return 'corruptJar';
      case 'server-mod-conflict':
        return 'modConflict';
      case 'server-missing-dep':
        return 'missingDep';
      case 'server-fabric-api-missing':
        return 'fabricApiMissing';
      case 'server-mixin-crash':
        return 'mixinCrash';
      case 'server-world-corrupt':
        return 'worldCorrupt';
      case 'server-session-lock':
        return 'sessionLock';
      case 'server-wrong-loader-mod':
        return 'wrongLoader';
      case 'server-disk-low':
        return 'diskLow';
      case 'server-crash-unknown':
        return 'crashUnknown';
      default:
        return null;
    }
  }

  const key = $derived(diag?.diagnosis ? patternKey(diag.diagnosis.pattern_id) : null);

  // The "Use port N" target: the backend-probed free port (never the current one
  // and verified bindable). Falls back to current+1 only if probing returned
  // nothing (whole scan window busy) — keeps the button functional regardless.
  const suggestedPort = $derived(diag?.suggested_port ?? (diag?.port_in_use ?? 25565) + 1);

  // Exit code for the crash-unknown fallback, shown to the user. NTSTATUS-style
  // negative codes (Windows process failures) read best in hex (e.g. 0xC0000142);
  // small positive codes stay decimal.
  const exitCodeLabel = $derived.by(() => {
    const c = diag?.exit_code;
    if (c == null) return null;
    return c < 0 ? `0x${(c >>> 0).toString(16).toUpperCase()}` : String(c);
  });

  // Client-mod checklist state
  let showChecklist = $state(false);
  let checked = $state<Record<string, boolean>>({});
  let busyRemove = $state(false);
  let removeError = $state<string | null>(null);

  // One-click pre-spawn fix state (accept EULA / stop orphan / change port).
  let busyFix = $state(false);
  let fixError = $state<string | null>(null);
  // Manual re-diagnose feedback (so the button never looks like it does nothing).
  let busyDiagnose = $state(false);
  // Neutral informational line (e.g. "couldn't find these mods automatically").
  let installInfo = $state<string | null>(null);

  async function runFix(fn: () => Promise<{ ok: boolean; error?: unknown }>) {
    busyFix = true;
    fixError = null;
    installInfo = null; // a prior unresolved-deps hint must not outlive this action
    try {
      const r = await fn();
      if (r.ok) {
        await serverState.diagnose(serverId);
      } else {
        fixError = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } catch (e) {
      fixError = formatError(e as Parameters<typeof formatError>[0]);
    } finally {
      busyFix = false;
    }
  }

  // Manual diagnose with visible feedback — re-run and confirm via a toast, so a
  // re-check that finds no change still reads as "it did something". Honest: only
  // claim success when the command actually succeeded.
  async function runDiagnose() {
    busyDiagnose = true;
    fixError = null;
    installInfo = null;
    try {
      const r = await serverState.diagnose(serverId);
      if (r.ok) {
        pushSuccess(get(t)('servers.diagnose.diagnoseDone'));
      } else {
        fixError = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } finally {
      busyDiagnose = false;
    }
  }

  // One-click metadata-driven quarantine of client-only mods (the proper fix for
  // a modpack server crashing on client mods). Dependency-safe in the backend.
  async function runQuarantine() {
    busyFix = true;
    fixError = null;
    installInfo = null;
    try {
      const r = await serverState.quarantineClientMods(serverId);
      if (r.ok) {
        const n = r.report?.disabled.length ?? 0;
        const kept = r.report?.kept_because_required.length ?? 0;
        let msg =
          n > 0
            ? `${get(t)('servers.diagnose.quarantined', { count: n })} ${get(t)('servers.diagnose.restartHint')}`
            : get(t)('servers.diagnose.quarantineNone');
        if (kept > 0) {
          // Surface the dependency-rescue — the headline safety behaviour.
          msg += ` ${get(t)('servers.diagnose.quarantineKeptRequired', { count: kept })}`;
        }
        pushSuccess(msg);
        await serverState.diagnose(serverId);
      } else {
        fixError = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } catch (e) {
      fixError = formatError(e as Parameters<typeof formatError>[0]);
    } finally {
      busyFix = false;
    }
  }

  // Install missing dependency mods — honest about partial/zero results instead
  // of clearing the banner on a silent no-op.
  async function runInstallMissingDep() {
    busyFix = true;
    fixError = null;
    installInfo = null;
    try {
      const r = await serverState.installMissingDep(serverId, diag?.conflict_mods ?? []);
      if (!r.ok) {
        fixError = formatError(r.error as Parameters<typeof formatError>[0]);
        return;
      }
      const installed = r.report?.installed ?? [];
      const unresolved = r.report?.unresolved ?? [];
      if (installed.length > 0) {
        pushSuccess(
          `${get(t)('servers.diagnose.installReportOk', { count: installed.length })} ${get(t)('servers.diagnose.restartHint')}`,
        );
        await serverState.diagnose(serverId);
      }
      if (unresolved.length > 0) {
        installInfo = get(t)('servers.diagnose.installReportUnresolved', {
          names: unresolved.join(', '),
        });
      }
    } catch (e) {
      fixError = formatError(e as Parameters<typeof formatError>[0]);
    } finally {
      busyFix = false;
    }
  }

  // Seed `checked` when client_mods change — pre-check high-confidence rows,
  // but only seed keys not yet present so user toggles are preserved.
  $effect(() => {
    const mods = diag?.client_mods ?? [];
    for (const f of mods) {
      if (!(f.filename in checked)) {
        checked[f.filename] = f.confidence === 'high';
      }
    }
  });

  function reasonKey(reason: string): 'manifestClient' | 'crash' {
    return reason === 'manifest_client' ? 'manifestClient' : 'crash';
  }

  async function removeSelected() {
    const mods: ClientModFinding[] = diag?.client_mods ?? [];
    const sel = mods.filter((f) => checked[f.filename]).map((f) => f.filename);
    if (sel.length === 0) return;
    busyRemove = true;
    removeError = null;
    try {
      // disable_mods (conflict/mixin) renames to *.disabled (reversible); the
      // client-only-crash path deletes. Pick the right command per repair tag.
      const r =
        diag?.server_repair === 'disable_mods'
          ? await serverState.disableMods(serverId, sel, diag?.log_signature ?? null)
          : await serverState.removeClientMods(serverId, sel, diag?.log_signature ?? null);
      if (r.ok) {
        pushSuccess(
          `${get(t)('servers.diagnose.removed', { count: sel.length })} ${get(t)('servers.diagnose.restartHint')}`,
        );
        showChecklist = false;
      } else {
        removeError = formatError(r.error as Parameters<typeof formatError>[0]);
      }
    } catch (e) {
      removeError = formatError(e as Parameters<typeof formatError>[0]);
    } finally {
      busyRemove = false;
    }
  }
</script>

{#if diag && diag.diagnosis && diag.status !== 'none' && diag.status !== 'handled' && !running && !dismissed}
  <!-- role="alert" so screen-reader users hear the diagnosis when it appears
       after a crash (it renders conditionally, not on mount). -->
  <div
    class="rounded-xl border border-warning-text bg-warning-bg p-3 text-warning-text"
    data-testid="server-diagnosis-banner"
    role="alert"
  >
    <div class="flex items-start gap-2">
      <Icon name="warning" class="mt-0.5 shrink-0 text-warning-text" />
      <div class="flex-1 min-w-0">
        <p class="font-semibold text-warning-text">
          {key ? $t(`servers.diagnose.${key}.title`) : diag.diagnosis.title}
        </p>
        <p class="mt-1 text-sm text-primary">
          {#if key === 'crashUnknown'}
            {$t('servers.diagnose.crashUnknown.explanation', { code: exitCodeLabel ?? '?' })}
          {:else if key}
            {$t(`servers.diagnose.${key}.explanation`)}
          {:else}
            {diag.diagnosis.explanation}
          {/if}
        </p>
        <p class="mt-1 text-sm text-primary">
          {key ? $t(`servers.diagnose.${key}.recommendation`) : diag.diagnosis.recommendation}
        </p>

        {#if diag.forge_skip_count != null && diag.forge_skip_count > 0}
          <p class="mt-1 text-xs text-warning-text/80">
            {$t('servers.diagnose.forgeSkipNote', { count: diag.forge_skip_count })}
          </p>
        {/if}

        <!-- Manual diagnose button always shown so the user can re-run -->
        <BusyButton
          class="btn-ghost btn-sm mt-2"
          data-testid="server-diagnose-btn"
          busy={busyDiagnose}
          onclick={() => void runDiagnose()}
        >
          {$t('servers.diagnose.diagnoseBtn')}
        </BusyButton>

        {#if diag.server_repair === 'remove_client_mods'}
          <BusyButton
            class="btn-warning btn-sm mt-2"
            data-testid="server-fix-quarantine"
            busy={busyFix}
            aria-label={$t('servers.diagnose.quarantineClientMods')}
            onclick={() => void runQuarantine()}
          >
            {$t('servers.diagnose.quarantineClientMods')}
          </BusyButton>
        {:else if diag.server_repair === 'accept_eula'}
          <BusyButton
            class="btn-warning btn-sm mt-2"
            data-testid="server-fix-accept-eula"
            busy={busyFix}
            aria-label={$t('servers.diagnose.fix.acceptEula')}
            onclick={() => void runFix(() => serverState.acceptEula(serverId))}
          >
            {$t('servers.diagnose.fix.acceptEula')}
          </BusyButton>
        {:else if diag.server_repair === 'stop_orphan_and_retry'}
          <BusyButton
            class="btn-warning btn-sm mt-2"
            data-testid="server-fix-stop-orphan"
            busy={busyFix}
            aria-label={$t('servers.diagnose.fix.stopOrphan')}
            onclick={() =>
              void runFix(() => serverState.stopOrphan(serverId, diag.orphan_pid ?? 0))}
          >
            {$t('servers.diagnose.fix.stopOrphan')}
          </BusyButton>
        {:else if diag.server_repair === 'change_port'}
          <BusyButton
            class="btn-warning btn-sm mt-2"
            data-testid="server-fix-change-port"
            busy={busyFix}
            aria-label={$t('servers.diagnose.fix.changePort', { port: suggestedPort })}
            onclick={() => void runFix(() => serverState.changePort(serverId, suggestedPort))}
          >
            {$t('servers.diagnose.fix.changePort', { port: suggestedPort })}
          </BusyButton>
        {:else if diag.server_repair === 'raise_heap'}
          <BusyButton
            class="btn-warning btn-sm mt-2"
            data-testid="server-fix-raise-heap"
            busy={busyFix}
            aria-label={$t('servers.diagnose.fix.raiseHeap', {
              mb: diag.suggested_heap_mb ?? 4096,
            })}
            onclick={() =>
              void runFix(() => serverState.raiseHeap(serverId, diag.suggested_heap_mb ?? 4096))}
          >
            {$t('servers.diagnose.fix.raiseHeap', { mb: diag.suggested_heap_mb ?? 4096 })}
          </BusyButton>
        {:else if diag.server_repair === 'lower_heap'}
          <BusyButton
            class="btn-warning btn-sm mt-2"
            data-testid="server-fix-lower-heap"
            busy={busyFix}
            aria-label={$t('servers.diagnose.fix.lowerHeap', {
              mb: diag.suggested_heap_mb ?? 2048,
            })}
            onclick={() =>
              void runFix(() => serverState.lowerHeap(serverId, diag.suggested_heap_mb ?? 2048))}
          >
            {$t('servers.diagnose.fix.lowerHeap', { mb: diag.suggested_heap_mb ?? 2048 })}
          </BusyButton>
        {:else if diag.server_repair === 'redownload_server_jar'}
          <BusyButton
            class="btn-warning btn-sm mt-2"
            data-testid="server-fix-redownload-jar"
            busy={busyFix}
            aria-label={$t('servers.diagnose.fix.redownloadJar')}
            onclick={() => void runFix(() => serverState.redownloadJar(serverId))}
          >
            {$t('servers.diagnose.fix.redownloadJar')}
          </BusyButton>
        {:else if diag.server_repair === 'install_missing_dep'}
          <BusyButton
            class="btn-warning btn-sm mt-2"
            data-testid="server-fix-install-dep"
            busy={busyFix}
            aria-label={$t('servers.diagnose.fix.installMissingDep', {
              count: diag.conflict_mods.length,
            })}
            onclick={() => void runInstallMissingDep()}
          >
            {$t('servers.diagnose.fix.installMissingDep', { count: diag.conflict_mods.length })}
          </BusyButton>
        {/if}

        {#if fixError}
          <p class="mt-2 text-sm text-danger" role="alert" data-testid="server-fix-error">
            {fixError}
          </p>
        {/if}

        {#if installInfo}
          <p class="mt-2 text-sm text-primary" role="status" data-testid="server-install-info">
            {installInfo}
          </p>
        {/if}

        {#if diag.status === 'actionable' && diag.client_mods.length > 0}
          <div class="mt-2">
            <button
              type="button"
              class="btn-ghost btn-sm"
              onclick={() => (showChecklist = !showChecklist)}
            >
              {$t('servers.diagnose.showClientMods')}
            </button>

            {#if showChecklist}
              <ul class="mt-2 space-y-1">
                {#each diag.client_mods as f (f.filename)}
                  <li class="flex items-center gap-2 text-sm">
                    <input
                      type="checkbox"
                      id="mod-{f.filename}"
                      checked={checked[f.filename] ?? false}
                      onchange={(e) => {
                        checked[f.filename] = (e.currentTarget as HTMLInputElement).checked;
                      }}
                      class="shrink-0"
                    />
                    <label for="mod-{f.filename}" class="flex-1 min-w-0">
                      <span class="font-mono text-xs">{f.filename}</span>
                      <span class="ml-2 text-xs text-secondary">
                        {$t(`servers.diagnose.reason.${reasonKey(f.reason)}`)}
                        ·
                        {$t(`servers.diagnose.confidence.${f.confidence}`)}
                      </span>
                    </label>
                  </li>
                {/each}
              </ul>

              <BusyButton
                class="btn-warning btn-sm mt-2"
                busy={busyRemove}
                disabled={!diag.client_mods.some((f) => checked[f.filename])}
                onclick={() => void removeSelected()}
              >
                {diag.server_repair === 'disable_mods'
                  ? $t('servers.diagnose.disableSelected')
                  : $t('servers.diagnose.removeSelected')}
              </BusyButton>
            {/if}
          </div>
        {/if}

        <!-- Conflict (B8): the loader cited mod ids but the classifier matched no
             installed jar to disable automatically — list them as guidance. -->
        {#if diag.server_repair === 'disable_mods' && diag.client_mods.length === 0 && diag.conflict_mods.length > 0}
          <div class="mt-2 text-sm text-primary" data-testid="server-fix-disable-mods">
            <p>{$t('servers.diagnose.conflictNamed')}</p>
            <ul class="mt-1 list-disc pl-5">
              {#each diag.conflict_mods as id (id)}
                <li class="font-mono text-xs">{id}</li>
              {/each}
            </ul>
          </div>
        {/if}

        {#if removeError}
          <p class="mt-2 text-sm text-danger" role="alert" data-testid="server-remove-error">
            {removeError}
          </p>
        {/if}
      </div>
      <button
        type="button"
        class="btn-icon -my-1 -mr-1 shrink-0 !text-warning-text hover:!bg-warning-text/10"
        aria-label={$t('common.dismissWarning')}
        use:tooltip={$t('common.dismissWarningTooltip')}
        onclick={() => signature && diagnosisDismiss.dismiss(`server:${serverId}`, signature)}
        data-testid="server-diagnosis-dismiss"><Icon name="close" size={16} /></button
      >
    </div>
  </div>
{/if}
