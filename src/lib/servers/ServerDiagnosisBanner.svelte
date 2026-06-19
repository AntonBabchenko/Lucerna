<script lang="ts">
  import { get } from 'svelte/store';
  import { t } from '$lib/i18n';
  import { formatError } from '$lib/ipc/format-error';
  import type { ClientModFinding } from '$lib/ipc/bindings';
  import { serverState } from '$lib/servers/server-state.svelte';
  import { pushSuccess } from '$lib/toasts/toasts.svelte';
  import { Icon } from '$lib/ui/icons';
  import BusyButton from '$lib/ui/BusyButton.svelte';

  let { serverId }: { serverId: string } = $props();

  const diag = $derived(serverState.diagnosisFor(serverId));

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
    | 'wrongLoader';

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
      default:
        return null;
    }
  }

  const key = $derived(diag?.diagnosis ? patternKey(diag.diagnosis.pattern_id) : null);

  // Client-mod checklist state
  let showChecklist = $state(false);
  let checked = $state<Record<string, boolean>>({});
  let busyRemove = $state(false);
  let removeError = $state<string | null>(null);

  // One-click pre-spawn fix state (accept EULA / stop orphan / change port).
  let busyFix = $state(false);
  let fixError = $state<string | null>(null);

  async function runFix(fn: () => Promise<{ ok: boolean; error?: unknown }>) {
    busyFix = true;
    fixError = null;
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

{#if diag && diag.diagnosis && diag.status !== 'none' && diag.status !== 'handled'}
  <div
    class="rounded border border-warning-text/30 bg-warning-bg p-3 text-warning-text"
    data-testid="server-diagnosis-banner"
  >
    <div class="flex items-start gap-2">
      <Icon name="warning" class="mt-0.5 shrink-0 text-warning-text" />
      <div class="flex-1 min-w-0">
        <p class="font-semibold text-warning-text">
          {key ? $t(`servers.diagnose.${key}.title`) : diag.diagnosis.title}
        </p>
        <p class="mt-1 text-sm text-primary">
          {key ? $t(`servers.diagnose.${key}.explanation`) : diag.diagnosis.explanation}
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
        <button
          type="button"
          class="btn-ghost btn-sm mt-2"
          onclick={() => void serverState.diagnose(serverId)}
        >
          {$t('servers.diagnose.diagnoseBtn')}
        </button>

        {#if diag.server_repair === 'accept_eula'}
          <BusyButton
            class="btn-warning btn-sm mt-2"
            data-testid="server-fix-accept-eula"
            busy={busyFix}
            onclick={() => void runFix(() => serverState.acceptEula(serverId))}
          >
            {$t('servers.diagnose.fix.acceptEula')}
          </BusyButton>
        {:else if diag.server_repair === 'stop_orphan_and_retry'}
          <BusyButton
            class="btn-warning btn-sm mt-2"
            data-testid="server-fix-stop-orphan"
            busy={busyFix}
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
            onclick={() =>
              void runFix(() => serverState.changePort(serverId, (diag.port_in_use ?? 25565) + 1))}
          >
            {$t('servers.diagnose.fix.changePort', { port: (diag.port_in_use ?? 25565) + 1 })}
          </BusyButton>
        {:else if diag.server_repair === 'raise_heap'}
          <BusyButton
            class="btn-warning btn-sm mt-2"
            data-testid="server-fix-raise-heap"
            busy={busyFix}
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
            onclick={() => void runFix(() => serverState.redownloadJar(serverId))}
          >
            {$t('servers.diagnose.fix.redownloadJar')}
          </BusyButton>
        {:else if diag.server_repair === 'install_missing_dep'}
          <BusyButton
            class="btn-warning btn-sm mt-2"
            data-testid="server-fix-install-dep"
            busy={busyFix}
            onclick={() =>
              void runFix(() => serverState.installMissingDep(serverId, diag.conflict_mods))}
          >
            {$t('servers.diagnose.fix.installMissingDep', { count: diag.conflict_mods.length })}
          </BusyButton>
        {/if}

        {#if fixError}
          <p class="mt-2 text-sm text-danger">{fixError}</p>
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
          <p class="mt-2 text-sm text-danger">{removeError}</p>
        {/if}
      </div>
    </div>
  </div>
{/if}
