<script lang="ts">
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import type { DepViolation, PreflightReport } from '$lib/ipc/bindings';
  import { depNameOf } from '$lib/mods/dep-names.svelte';
  import { hasBlocking, violationKey } from './preflight.svelte';
  import {
    depDisplayName,
    isFixable,
    isRangeRemediable,
    type PlanSide,
    type PlanState,
    planOffers,
    type ViolationAction,
    violationAction,
    violationMessage,
  } from './violation-view';

  // «What stops the game» (spec §6.2): the pre-flight's blocking reasons, each
  // with ↗ to its row and its fix. The same panel is the Play gate's list.
  let {
    report,
    instanceId = null,
    onInstallMissing = () => {},
    onEnableProvider = () => {},
    onChooseVersion = () => {},
    ownVersionOpener = undefined,
    onFindAlternative = () => {},
    onOpenModPage = () => {},
    onJumpToDependent = undefined,
    onFixAll = undefined,
    fixAllBusy = false,
    onMigrate = undefined,
    migrateCount = 0,
    busyKeys = new Set<string>(),
    deadEndKeys = new Set<string>(),
    plans = new Map<string, PlanState>(),
    onPlan = () => {},
    onApplyPlan = () => {},
    showRowActions = true,
    showHeader = true,
  }: {
    report: PreflightReport | null;
    // Whose name store names the dependencies (spec §5.3). Every row is named by ONE rule
    // (`depDisplayName`) — the provider's own name from the report, else the name resolved for
    // this dependent, else the raw loader id, per row — the same on the Installed tab and at the
    // Play gate, so the two never name one mod two ways. Reading the store makes no call.
    instanceId?: string | null;
    onInstallMissing?: (v: DepViolation) => void;
    // `required_disabled`: switch the disabled provider back on (mod-ops asks
    // first when it has disabled requirements of its own).
    onEnableProvider?: (v: DepViolation) => void;
    onChooseVersion?: (v: DepViolation) => void;
    // `platform_mismatch`: another build of the dependent ITSELF, from its own version list —
    // the row's «Choose version» (spec §6.2). The host returns how to open that list, or null
    // when the mod has none to pick from (a manual jar): no dead button.
    ownVersionOpener?: (v: DepViolation) => (() => void) | null;
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
    // Row keys (violationKey) mid-remediation / where the planner found no
    // build either side that fixes the conflict. Pass a SvelteSet for live
    // updates — a plain Set is read once.
    busyKeys?: Set<string>;
    deadEndKeys?: Set<string>;
    // The two-sided planner per row key (spec §5.4, §6.5) — a SvelteMap for
    // live updates. «Fix…» asks for a plan (network only on that click); a
    // ready plan's offers apply one side each, a breaking one only on its own
    // click (D8).
    plans?: Map<string, PlanState>;
    onPlan?: (v: DepViolation) => void;
    onApplyPlan?: (v: DepViolation, side: PlanSide) => void;
    // The launch gate mutes per-row actions (it repairs through its own
    // button), so it passes false to hide them.
    showRowActions?: boolean;
    // The launch gate's dialog title already says «What stops the game»; a
    // second heading inside would repeat it, so the gate hides this one.
    showHeader?: boolean;
  } = $props();

  const nameOf = (v: DepViolation): string =>
    depDisplayName(v, depNameOf(instanceId, v.dependent_sha1, v.dep_id));

  // Ids for the text a row's buttons are described by — the planner's note, a
  // change's «Breaks:» — per row index (and offer side).
  const uid = $props.id();

  // Blocking rows only, by the gate's own predicate: while a self-completing
  // pack is still fetching files its complaints are advisory, and a panel
  // titled «What stops the game» must not list them.
  const violations = $derived(report && hasBlocking(report) ? report.violations : []);
  const blocking = $derived(violations.length > 0);
  const fixableCount = $derived(violations.filter(isFixable).length);
  const showFixAll = $derived(!!onFixAll && fixableCount > 0);
  const showMigrate = $derived(!!onMigrate && migrateCount > 0);
</script>

{#if blocking || (showHeader && showMigrate)}
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
    {#if showHeader}
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
    {/if}
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
        {#each violations as v, i (violationKey(v))}
          {@const key = violationKey(v)}
          {@const action = violationAction(v)}
          <!-- A platform mismatch has no automatic fix (spec §6.4): another build of THIS mod,
               as its row offers. -->
          {@const openOwn =
            showRowActions && v.kind === 'platform_mismatch'
              ? (ownVersionOpener?.(v) ?? null)
              : null}
          {@const hasActions = showRowActions && (action !== 'none' || openOwn !== null)}
          <!-- Three columns: the icon, the reason with its fixes, ↗. The row lines them up by
               baseline, so the icon sits on the reason's first line whatever shares that line. The
               reason keeps a readable width (`basis-72`); its fixes sit beside it while both fit
               and wrap under it — one group, which wraps in itself — when they do not. `flex-1`
               gave it a basis of 0: fixed-width offers took the line and left it a word per line
               (plan §5c, screenshot n03b). -->
          <div
            class="px-4 py-2.5 flex items-baseline gap-3 border-b border-border-subtle last:border-b-0"
            data-testid="preflight-row"
            data-violation-key={key}
          >
            <span class="shrink-0 text-sm"
              ><Icon name="circleX" class="inline-block align-middle text-danger" /></span
            >
            <div class="flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-1">
              <span
                class="min-w-0 grow basis-72 text-sm text-primary"
                data-testid="preflight-row-text">{violationMessage($t, v, nameOf(v))}</span
              >
              {#if hasActions}
                <div
                  class="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1"
                  data-testid="preflight-row-actions"
                >
                  {@render rowActions(v, i, key, action, openOwn)}
                </div>
              {/if}
            </div>
            {#if showRowActions && onJumpToDependent}
              <button
                type="button"
                class="btn-icon btn-icon-sm shrink-0 self-center"
                aria-label={$t('mods.deps.jumpToTitle', { name: v.dependent_name })}
                use:tooltip={$t('mods.deps.jumpToTitle', { name: v.dependent_name })}
                onclick={() => onJumpToDependent?.(v)}
                ><Icon name="arrowUpRight" size={14} /></button
              >
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<!-- What fixes one row, in the order it offers them. -->
{#snippet rowActions(
  v: DepViolation,
  i: number,
  key: string,
  action: ViolationAction,
  openOwn: (() => void) | null,
)}
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
  {:else if action === 'plan'}
    {@const plan = plans.get(key)}
    {@const noteId = `${uid}-${i}-note`}
    <!-- A version conflict (a range either way, or an incompatibility):
         «Fix…» asks the two-sided planner. A look that failed is not
         «no version» (spec §9); both sides empty is the honest dead end.
         What the planner said describes the buttons after it, so it is
         heard where focus lands, not only seen. -->
    {#if busyKeys.has(key)}
      <Spinner size="sm" class="shrink-0 text-secondary" />
    {:else if plan?.status === 'loading'}
      <Spinner
        size="sm"
        labelPlacement="right"
        label={$t('mods.preflight.planLooking')}
        class="shrink-0 text-secondary"
      />
    {:else if deadEndKeys.has(key)}
      <span id={noteId} class="shrink-0 text-xs text-secondary">
        {$t('mods.preflight.noCompatible')}
      </span>
      {#if v.provider_project !== null}
        <button
          type="button"
          class="btn-link text-xs shrink-0"
          aria-describedby={noteId}
          onclick={() => onOpenModPage(v)}
        >
          {$t('mods.preflight.openModPage')}
        </button>
      {/if}
      <button
        type="button"
        class="btn-link text-xs shrink-0"
        aria-describedby={noteId}
        use:tooltip={{ text: $t('mods.preflight.findAlternativeTip'), describe: false }}
        onclick={() => onFindAlternative(v)}
      >
        {$t('mods.preflight.findAlternative')}
      </button>
    {:else}
      {#if plan?.status === 'ready'}
        {#each planOffers($t, v, plan.plan, nameOf(v)) as o (o.side)}
          {@const breaksId = `${uid}-${i}-${o.side}-breaks`}
          <!-- What a change would break is said beside it and heard with it;
               it is never the default, and it takes its own click (D8). The
               offer and its note are one item of the fixes' line, which wraps
               as a whole: a note on a line of its own read as the next offer's
               (plan §5d M2). A unit wider than the whole line puts the note
               under its own button. -->
          <span
            class="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1"
            data-testid="preflight-plan-offer"
          >
            <button
              type="button"
              class="{o.primary ? 'btn-primary' : 'btn-secondary'} btn-xs shrink-0"
              data-testid="preflight-plan-{o.side}"
              aria-describedby={o.breaks.length > 0 ? breaksId : undefined}
              onclick={() => onApplyPlan(v, o.side)}
            >
              {o.label}
            </button>
            {#if o.breaks.length > 0}
              <span
                id={breaksId}
                class="min-w-0 text-xs text-warning-text"
                data-testid="preflight-plan-breaks"
              >
                {$t('mods.preflight.planBreaks', { names: o.breaks.join(', ') })}
              </span>
            {/if}
          </span>
        {/each}
      {:else}
        {#if plan?.status === 'failed'}
          <span id={noteId} class="min-w-0 text-xs text-secondary">
            {$t('mods.preflight.planFailed', { reason: plan.message })}
          </span>
        {/if}
        <button
          type="button"
          class="btn-secondary btn-xs shrink-0"
          aria-describedby={plan?.status === 'failed' ? noteId : undefined}
          onclick={() => onPlan(v)}
        >
          {$t('mods.preflight.fixPlan')}
        </button>
      {/if}
      <!-- The manual path, for a range only: for an incompatibility the
           picker's "fits range" marks exactly the builds that clash. -->
      {#if isRangeRemediable(v) && v.provider_project !== null}
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
  {:else if openOwn}
    <button
      type="button"
      class="btn-secondary btn-xs shrink-0"
      use:tooltip={{ text: $t('mods.preflight.chooseVersionTip'), describe: false }}
      onclick={openOwn}
    >
      {$t('mods.preflight.chooseVersion')}
    </button>
  {/if}
{/snippet}
