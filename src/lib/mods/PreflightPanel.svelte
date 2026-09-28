<script lang="ts">
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import type { DepViolation, PreflightReport } from '$lib/ipc/bindings';
  import { depNameOf } from '$lib/mods/dep-names.svelte';
  import { hasBlocking, isRangeRemediable, violationKey } from './preflight.svelte';
  import { isFixable, violationAction, violationMessage } from './violation-view';

  // «What stops the game» (spec §6.2): the pre-flight's blocking reasons, each
  // with ↗ to its row and its fix. The same panel is the Play gate's list.
  let {
    report,
    instanceId = null,
    depName = undefined,
    onUpdate = () => {},
    onInstallMissing = () => {},
    onEnableProvider = () => {},
    onChooseVersion = () => {},
    onFindAlternative = () => {},
    onOpenModPage = () => {},
    onJumpToDependent = undefined,
    onFixAll = undefined,
    fixAllBusy = false,
    onMigrate = undefined,
    migrateCount = 0,
    busyKeys = new Set<string>(),
    deadEndKeys = new Set<string>(),
    showRowActions = true,
  }: {
    report: PreflightReport | null;
    // The dependency's display name. The host's lookup comes first (the
    // Installed tab also knows its own rows' names); where it has none — or
    // there is no host lookup, the launch gate's case — the dependent-scoped
    // name store is read (spec §5.3): names already resolved, never a network
    // call between the user and Play. Unresolved → the raw loader id, per row.
    instanceId?: string | null;
    depName?: (v: DepViolation) => string | null;
    onUpdate?: (v: DepViolation) => void;
    onInstallMissing?: (v: DepViolation) => void;
    // `required_disabled`: switch the disabled provider back on (mod-ops asks
    // first when it has disabled requirements of its own).
    onEnableProvider?: (v: DepViolation) => void;
    onChooseVersion?: (v: DepViolation) => void;
    onFindAlternative?: (v: DepViolation) => void;
    onOpenModPage?: (v: DepViolation) => void;
    // ↗ to the dependent's own row — only the Installed tab has a list.
    onJumpToDependent?: (v: DepViolation) => void;
    // «Fix all (N)» over the fixable rows. The gate repairs through its own
    // primary instead and leaves it unset.
    onFixAll?: () => void;
    fixAllBusy?: boolean;
    // Bulk migration entry, on compat's own count — NOT the violation count: a
    // jar can be incompatible with no dependency violation, so this can show
    // with no rows at all. Only the Installed tab passes it.
    onMigrate?: () => void;
    migrateCount?: number;
    // Row keys (violationKey) mid-remediation / with no satisfying version.
    // Pass a SvelteSet for live updates — a plain Set is read once.
    busyKeys?: Set<string>;
    deadEndKeys?: Set<string>;
    // The launch gate mutes per-row actions (it repairs through its own
    // button), so it passes false to hide them.
    showRowActions?: boolean;
  } = $props();

  const nameOf = (v: DepViolation): string =>
    depName?.(v) ??
    (instanceId ? depNameOf(instanceId, v.dependent_sha1, v.dep_id) : null) ??
    v.dep_id;

  // Blocking rows only, by the gate's own predicate: while a self-completing
  // pack is still fetching files its complaints are advisory, and a panel
  // titled «What stops the game» must not list them.
  const violations = $derived(report && hasBlocking(report) ? report.violations : []);
  const blocking = $derived(violations.length > 0);
  const fixableCount = $derived(violations.filter(isFixable).length);
  const showFixAll = $derived(!!onFixAll && fixableCount > 0);
  const showMigrate = $derived(!!onMigrate && migrateCount > 0);
</script>

{#if blocking || showMigrate}
  <!-- The surface with a danger border and a red icon, never the bg-danger-bg
       box: danger text on that box misses AA in both themes (DESIGN.md Known
       gaps). With only incompatibilities left nothing stops the game, so the
       panel says what is true instead — amber, «Some mods may not work». -->
  <div
    class="rounded-xl border bg-surface overflow-hidden mb-3 {blocking
      ? 'border-danger'
      : 'border-warning-text'}"
    data-testid="preflight-panel"
  >
    <div
      class="px-4 py-2.5 font-semibold text-primary flex flex-wrap items-center gap-2"
      class:border-b={blocking}
      class:border-border-subtle={blocking}
    >
      <Icon
        name={blocking ? 'circleX' : 'warning'}
        class="shrink-0 {blocking ? 'text-danger' : 'text-warning-text'}"
      />
      <span class="flex-1">
        {$t(blocking ? 'mods.preflight.panelTitle' : 'mods.preflight.panelTitleWarnOnly')}
      </span>
      {#if showFixAll}
        <BusyButton
          class="btn-primary btn-xs shrink-0"
          busy={fixAllBusy}
          data-testid="preflight-fix-all"
          onclick={() => onFixAll?.()}
        >
          {$t('mods.preflight.fixAll', { count: fixableCount })}
        </BusyButton>
      {/if}
      {#if showMigrate}
        <!-- Scoped to the incompatibility class by its copy: the panel also
             lists missing dependencies, which migration does not touch, so a
             whole-panel "fix" here would over-promise. -->
        <button
          type="button"
          class="btn-secondary btn-xs shrink-0"
          data-testid="preflight-migrate-btn"
          onclick={() => onMigrate?.()}
        >
          {$t('mods.preflight.fixIncompatible', { count: migrateCount })}
        </button>
      {/if}
    </div>
    {#if blocking}
      <!-- Cap the row list and let it scroll: a long list must not push the
           panel — or, in the launch gate, the dialog's footer — past the window
           edge. A scrollable region must be focusable so keyboard users can
           scroll it (WCAG 2.1.1); the noninteractive-tabindex rule is a false
           positive here. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        class="max-h-[min(22rem,45vh)] overflow-y-auto"
        tabindex="0"
        role="region"
        aria-label={$t('mods.preflight.panelTitle')}
        data-testid="preflight-scroll"
      >
        {#each violations as v (violationKey(v))}
          {@const key = violationKey(v)}
          {@const action = violationAction(v)}
          <div
            class="px-4 py-2.5 flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-border-subtle last:border-b-0"
            data-testid="preflight-row"
            data-violation-key={key}
          >
            <Icon name="circleX" class="text-danger shrink-0" />
            <span class="flex-1 min-w-0 text-sm text-primary"
              >{violationMessage($t, v, nameOf(v))}</span
            >
            {#if showRowActions}
              {#if action === 'enable'}
                <button
                  type="button"
                  class="btn-secondary btn-xs shrink-0"
                  use:tooltip={{ text: $t('mods.preflight.enableTip'), describe: false }}
                  onclick={() => onEnableProvider(v)}
                >
                  {$t('mods.preflight.enable')}
                </button>
              {:else if action === 'install'}
                <button
                  type="button"
                  class="btn-secondary btn-xs shrink-0"
                  use:tooltip={{ text: $t('mods.preflight.installTip'), describe: false }}
                  onclick={() => onInstallMissing(v)}
                >
                  {$t('mods.preflight.install', { dep: nameOf(v) })}
                </button>
              {:else if isRangeRemediable(v) && v.provider_project !== null}
                {#if busyKeys.has(key)}
                  <Spinner size="sm" class="shrink-0 text-secondary" />
                {:else if deadEndKeys.has(key)}
                  <span class="shrink-0 text-xs text-secondary">
                    {$t('mods.preflight.noCompatible')}
                  </span>
                  <button
                    type="button"
                    class="btn-link text-xs shrink-0"
                    onclick={() => onOpenModPage(v)}
                  >
                    {$t('mods.preflight.openModPage')}
                  </button>
                  <button
                    type="button"
                    class="btn-link text-xs shrink-0"
                    use:tooltip={{ text: $t('mods.preflight.findAlternativeTip'), describe: false }}
                    onclick={() => onFindAlternative(v)}
                  >
                    {$t('mods.preflight.findAlternative')}
                  </button>
                {:else}
                  <button
                    type="button"
                    class="btn-secondary btn-xs shrink-0"
                    use:tooltip={{ text: $t('mods.preflight.updateTip'), describe: false }}
                    onclick={() => onUpdate(v)}
                  >
                    {$t('mods.preflight.update')}
                  </button>
                  <button
                    type="button"
                    class="btn-link text-xs shrink-0"
                    use:tooltip={{ text: $t('mods.preflight.chooseVersionTip'), describe: false }}
                    onclick={() => onChooseVersion(v)}
                  >
                    {$t('mods.preflight.chooseVersion')}
                  </button>
                {/if}
              {/if}
              {#if onJumpToDependent}
                <button
                  type="button"
                  class="btn-icon btn-icon-sm shrink-0"
                  aria-label={$t('mods.deps.jumpToTitle', { name: v.dependent_name })}
                  use:tooltip={$t('mods.deps.jumpToTitle', { name: v.dependent_name })}
                  onclick={() => onJumpToDependent?.(v)}
                  ><Icon name="arrowUpRight" size={14} /></button
                >
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}
