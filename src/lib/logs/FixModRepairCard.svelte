<script lang="ts">
  import { t } from '$lib/i18n';
  import type { RepairChoice, RepairPlan } from '$lib/ipc/bindings';
  import Spinner from '$lib/ui/Spinner.svelte';

  let {
    plan,
    busy = false,
    onConfirm,
    onCancel,
  }: {
    plan: Extract<RepairPlan, { kind: 'install_fix_mod' }>;
    busy?: boolean;
    onConfirm: (choice: RepairChoice) => void;
    onCancel: () => void;
  } = $props();

  const sourceLabel = $derived(plan.source === 'curseforge' ? 'CurseForge' : 'Modrinth');

  function projectUrl(): string {
    return plan.source === 'curseforge'
      ? `https://www.curseforge.com/minecraft/mc-mods/${plan.slug}`
      : `https://modrinth.com/mod/${plan.slug}`;
  }

  function openProject(): void {
    const url = projectUrl();
    if (!url.startsWith('https://')) return;
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
  }

  function install(): void {
    const target = plan.install;
    if (!target) return;
    onConfirm({
      kind: 'install_fix_mod',
      source: target.source,
      project_id: target.project_id,
      version_id: target.version_id,
    });
  }
</script>

<div class="mt-3 rounded border border-warning-text/40 bg-surface p-3" data-testid="fix-mod-card">
  <p class="text-sm">
    {$t('logs.repair.fixMod.intro')}
    <span class="font-semibold">{plan.title}</span>
  </p>
  <p class="mt-1 text-xs text-muted">
    {sourceLabel}{#if plan.version_label}
      · {plan.version_label}{/if}
    · {$t('logs.repair.fixMod.thirdPartyNote')}
  </p>

  <div class="mt-3 flex flex-wrap items-center gap-2">
    {#if plan.install}
      <button
        type="button"
        class="btn-primary btn-sm"
        data-testid="fix-mod-install"
        disabled={busy}
        onclick={install}
      >
        {#if busy}
          <span class="inline-flex items-center gap-1.5">
            <Spinner size="sm" />
            {$t('logs.repair.working')}
          </span>
        {:else}
          {$t('logs.repair.fixMod.install')}
        {/if}
      </button>
    {:else}
      <p class="text-xs text-muted" data-testid="fix-mod-degraded">
        {$t('logs.repair.fixMod.degraded')}
      </p>
      <button
        type="button"
        class="btn-secondary btn-sm"
        data-testid="fix-mod-open"
        onclick={openProject}
      >
        {$t('logs.repair.fixMod.openProject')}
      </button>
    {/if}
    <button
      type="button"
      class="btn-secondary btn-sm"
      data-testid="fix-mod-cancel"
      disabled={busy}
      onclick={onCancel}
    >
      {$t('common.cancel')}
    </button>
  </div>
</div>
