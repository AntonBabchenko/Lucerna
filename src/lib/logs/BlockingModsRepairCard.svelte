<script lang="ts">
  import { t } from '$lib/i18n';
  import { commands } from '$lib/ipc/bindings';
  import type { RepairPlan } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { pushSuccess, pushWarning } from '$lib/toasts/toasts.svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import Spinner from '$lib/ui/Spinner.svelte';

  // The diagnoser found the client carries enforced-channel mods the server
  // lacks (a "server side" reject). Each is installed; disabling it — reversible —
  // lets the user reconnect. Per-mod Disable mirrors the install card's per-mod
  // Install, and reuses execute_repair's existing DisableMod path (no parallel
  // backend). A `breaks` warning flags mods that depend on the one being disabled.
  let {
    plan,
    instanceId,
    onClose,
  }: {
    plan: Extract<RepairPlan, { kind: 'disable_blocking_mods' }>;
    instanceId: string;
    onClose: () => void;
  } = $props();

  // sha1s whose disable is in flight, and those already disabled this session, so
  // the button can't be re-clicked (a second disable would error / re-toast).
  const disablingSha1s = new SvelteSet<string>();
  const disabledSha1s = new SvelteSet<string>();

  // The card's job is done once every listed mod has been disabled — switch the
  // footer to a clear "you're done, reconnect" signal.
  const allDisabled = $derived(
    plan.mods.length > 0 && plan.mods.every((m) => disabledSha1s.has(m.sha1)),
  );

  async function disableMod(sha1: string, name: string) {
    disablingSha1s.add(sha1);
    try {
      const res = await commands.executeRepair(instanceId, { kind: 'disable_mod', sha1 });
      if (res.status === 'ok') {
        disabledSha1s.add(sha1);
        pushSuccess($t('logs.repair.blockingMods.disabledToast', { name }));
      } else {
        pushWarning(
          $t('logs.repair.blockingMods.failedToast', { name, error: formatError(res.error) }),
        );
      }
    } finally {
      disablingSha1s.delete(sha1);
    }
  }
</script>

<div
  class="mt-3 rounded border border-warning-text/40 bg-surface p-3"
  data-testid="blocking-mods-card"
>
  <p class="text-sm font-semibold">{$t('logs.repair.blockingMods.title')}</p>
  <p class="mt-1 text-xs text-muted" data-testid="blocking-intro">
    {$t('logs.repair.blockingMods.intro')}
  </p>

  <div class="mt-3 flex flex-col gap-2">
    {#each plan.mods as m (m.sha1)}
      <div class="text-sm">
        <span class="font-mono">{m.mod_id}</span>
        <details class="mt-1" data-testid={`blocking-disclosure-${m.sha1}`}>
          <summary class="cursor-pointer text-xs text-secondary hover:text-primary">
            {$t('logs.repair.blockingMods.disableQuestion')}
          </summary>
          <div class="mt-2 flex flex-col gap-2">
            <p class="text-xs text-muted">{$t('logs.repair.blockingMods.disableHint')}</p>
            {#if m.breaks.length > 0}
              <p
                class="text-xs text-warning-text/80"
                data-testid={`blocking-breaks-${m.sha1}`}
              >
                {$t('logs.repair.blockingMods.breaks', { names: m.breaks.join(', ') })}
              </p>
            {/if}
            <button
              type="button"
              class="btn-warning btn-xs self-start"
              data-testid={`blocking-disable-${m.sha1}`}
              aria-label={`${$t('logs.repair.blockingMods.disable')} ${m.mod_id}`}
              disabled={disablingSha1s.has(m.sha1) || disabledSha1s.has(m.sha1)}
              onclick={() => void disableMod(m.sha1, m.mod_id)}
            >
              {#if disabledSha1s.has(m.sha1)}
                {$t('logs.repair.blockingMods.disabledLabel')}
              {:else if disablingSha1s.has(m.sha1)}
                <span class="inline-flex items-center gap-1.5"
                  ><Spinner size="sm" label={$t('logs.repair.working')} />{$t(
                    'logs.repair.working',
                  )}</span
                >
              {:else}
                {$t('logs.repair.blockingMods.disable')}
              {/if}
            </button>
          </div>
        </details>
      </div>
    {/each}
  </div>

  {#if allDisabled}
    <p class="mt-3 text-xs font-semibold text-success" data-testid="blocking-all-disabled">
      {$t('logs.repair.blockingMods.allDisabled')}
    </p>
  {:else}
    <p class="mt-3 text-xs text-muted">{$t('logs.repair.blockingMods.reconnectHint')}</p>
  {/if}

  <div class="mt-3 flex items-center gap-2">
    <button
      type="button"
      class="btn-secondary btn-sm"
      data-testid="blocking-cancel"
      onclick={onClose}
    >
      {$t('common.cancel')}
    </button>
  </div>
</div>
