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
  function patternKey(patternId: string): 'clientOnly' | 'portInUse' | 'eula' | null {
    if (patternId === 'server-client-only-mod-crash') return 'clientOnly';
    if (patternId === 'server-port-in-use') return 'portInUse';
    if (patternId === 'server-eula-not-accepted') return 'eula';
    return null;
  }

  const key = $derived(diag?.diagnosis ? patternKey(diag.diagnosis.pattern_id) : null);

  // Client-mod checklist state
  let showChecklist = $state(false);
  let checked = $state<Record<string, boolean>>({});
  let busyRemove = $state(false);
  let removeError = $state<string | null>(null);

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
      const r = await serverState.removeClientMods(serverId, sel, diag?.log_signature ?? null);
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
                {$t('servers.diagnose.removeSelected')}
              </BusyButton>
            {/if}
          </div>
        {/if}

        {#if removeError}
          <p class="mt-2 text-sm text-danger">{removeError}</p>
        {/if}
      </div>
    </div>
  </div>
{/if}
