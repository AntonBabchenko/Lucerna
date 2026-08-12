<script lang="ts">
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import Spinner from '$lib/ui/Spinner.svelte';
  import { tooltip } from '$lib/ui/tooltip';
  import type { DepViolation, PreflightReport } from '$lib/ipc/bindings';
  import { formatRange, isSoftRange } from './range-format';
  import { isRangeRemediable } from './preflight.svelte';

  let {
    report,
    onUpdate,
    onInstallMissing = () => {},
    onChooseVersion = () => {},
    onFindAlternative = () => {},
    onOpenModPage = () => {},
    onMigrate,
    migrateCount = 0,
    busyKeys = new Set<string>(),
    deadEndKeys = new Set<string>(),
    showRowActions = true,
  }: {
    report: PreflightReport | null;
    onUpdate: (v: DepViolation) => void;
    onInstallMissing?: (v: DepViolation) => void;
    onChooseVersion?: (v: DepViolation) => void;
    onFindAlternative?: (v: DepViolation) => void;
    onOpenModPage?: (v: DepViolation) => void;
    // Bulk migration entry. When supplied AND `migrateCount > 0`, a header
    // button opens the instance-wide migration plan (the same dialog the
    // per-row "Fix" opens). Only the Installed tab passes these — the launch
    // gate leaves them unset, so its header stays a plain title.
    onMigrate?: () => void;
    // Authoritative count of incompatible mods, from `compat.incompatibleCount`
    // — NOT the violation count. The two overlap but a jar can be
    // platform-incompatible with no dependency-graph violation, and the button
    // must follow the compat count so it appears exactly when there is
    // something to migrate.
    migrateCount?: number;
    // Row keys (violationKey) currently mid-remediation / with no satisfying
    // version. Pass a SvelteSet for live updates — a plain Set is read once and
    // won't reactively re-render the row on mutation.
    busyKeys?: Set<string>;
    deadEndKeys?: Set<string>;
    // The launch gate mutes per-row actions (it remediates via its own
    // "Update & launch" batch button), so it passes false to hide them.
    showRowActions?: boolean;
  } = $props();

  const violations = $derived(report?.violations ?? []);
  // The migration entry is offered on its own signal, independent of the
  // dependency-graph violations that fill the row list. This is why the panel
  // can show a header with no rows: incompatible mods without a dep violation.
  const showMigrate = $derived(!!onMigrate && migrateCount > 0);
  const rowKey = (v: DepViolation): string => `${v.dependent_sha1}:${v.dep_id}`;

  /**
   * One sentence per violation kind. The range is rendered in plain language —
   * raw Maven bracket notation (`(,6.0.9]`) is not something a player can read,
   * and for an incompatibility it would read backwards.
   */
  function rowMessage(v: DepViolation): string {
    const dep = v.dep_display_name ?? v.dep_id;
    if (v.kind === 'missing_required') {
      return $t('mods.preflight.missing', { dependent: v.dependent_name, dep });
    }
    // An incompatibility fires when the installed version is INSIDE the
    // declared range, so a range that constrains nothing fires for every
    // version. Naming versions there would contradict itself — "incompatible
    // with X 1.2 recommended (any version works)" — so drop the range instead.
    // Only this kind can reach a soft range: the other two need a Violated
    // verdict, which a soft range never produces.
    if (v.kind === 'incompatible_installed' && isSoftRange(v.needed_desc)) {
      return $t('mods.preflight.incompatibleWithAny', {
        dependent: v.dependent_name,
        dep,
        installed: v.installed_version ?? '',
      });
    }
    const key =
      v.kind === 'incompatible_installed'
        ? 'mods.preflight.incompatibleWith'
        : v.kind === 'optional_out_of_range'
          ? 'mods.preflight.optionalOutOfRange'
          : 'mods.preflight.outOfRange';
    return $t(key, {
      dependent: v.dependent_name,
      dep,
      needed: formatRange($t, v.needed_desc),
      installed: v.installed_version ?? '',
    });
  }
</script>

{#if violations.length > 0 || showMigrate}
  <div
    class="rounded-xl border border-warning-text bg-warning-bg overflow-hidden mb-3"
    data-testid="preflight-panel"
  >
    <div
      class="px-4 py-2.5 font-semibold text-warning-text border-b border-warning-text
        flex items-center gap-2"
      class:border-b-0={violations.length === 0}
    >
      <Icon name="warning" class="text-warning-text" />
      <span class="flex-1">{$t('mods.preflight.panelTitle')}</span>
      {#if showMigrate}
        <!-- Scoped to the incompatibility class by its copy ("Fix incompatible
             mods"): the panel also lists missing dependencies, which migration
             does not touch, so a whole-panel "fix" would over-promise. Opens
             the same instance-wide plan as each row's Fix button. -->
        <button
          type="button"
          class="shrink-0 text-xs font-medium px-2.5 py-1 rounded
          border border-warning-text text-warning-text hover:bg-warning-text/10
          focus-visible:outline focus-visible:outline-2 focus-visible:outline-warning-text"
          data-testid="preflight-migrate-btn"
          onclick={() => onMigrate?.()}
        >
          {$t('mods.migration.openBtn')}
        </button>
      {/if}
    </div>
    {#if violations.length > 0}
      <!-- Cap the row list and let it scroll: a long violation list (many mods)
           must not push the panel — or, in the launch gate, the dialog's footer
           buttons — past the window edge. Viewport-relative so it adapts to a
           compact window. -->
      <!-- A scrollable region must be focusable so keyboard users can scroll it
           (WCAG 2.1.1); the noninteractive-tabindex rule is a false positive here. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div
        class="max-h-[min(22rem,45vh)] overflow-y-auto"
        tabindex="0"
        role="region"
        aria-label={$t('mods.preflight.panelTitle')}
        data-testid="preflight-scroll"
      >
        {#each violations as v (v.dependent_sha1 + ':' + v.dep_id)}
          <div
            class="px-4 py-2.5 flex items-center gap-3 border-b border-warning-text/30 last:border-b-0"
            data-testid="preflight-row"
          >
            <Icon name="warning" class="text-warning-text shrink-0" />
            <span class="flex-1 text-sm text-warning-text">
              {rowMessage(v)}
            </span>
            {#if showRowActions && v.kind === 'missing_required'}
              <button
                type="button"
                class="shrink-0 text-xs font-medium px-2 py-1 rounded
              border border-warning-text text-warning-text hover:bg-warning-text/10
              focus-visible:outline focus-visible:outline-2 focus-visible:outline-warning-text"
                use:tooltip={{ text: $t('mods.preflight.installTip'), describe: false }}
                onclick={() => onInstallMissing(v)}
              >
                {$t('mods.preflight.install', { dep: v.dep_display_name ?? v.dep_id })}
              </button>
            {:else if showRowActions && isRangeRemediable(v) && v.provider_project !== null}
              {@const key = rowKey(v)}
              {#if busyKeys.has(key)}
                <Spinner size="sm" class="shrink-0 text-warning-text" />
              {:else if deadEndKeys.has(key)}
                <span class="shrink-0 text-xs text-warning-text">
                  {$t('mods.preflight.noCompatible')}
                </span>
                <button
                  type="button"
                  class="shrink-0 text-xs underline text-warning-text hover:opacity-80
                focus-visible:outline focus-visible:outline-2 focus-visible:outline-warning-text"
                  onclick={() => onOpenModPage(v)}
                >
                  {$t('mods.preflight.openModPage')}
                </button>
                <button
                  type="button"
                  class="shrink-0 text-xs underline text-warning-text hover:opacity-80
                focus-visible:outline focus-visible:outline-2 focus-visible:outline-warning-text"
                  use:tooltip={{ text: $t('mods.preflight.findAlternativeTip'), describe: false }}
                  onclick={() => onFindAlternative(v)}
                >
                  {$t('mods.preflight.findAlternative')}
                </button>
              {:else}
                <button
                  type="button"
                  class="shrink-0 text-xs font-medium px-2 py-1 rounded
                border border-warning-text text-warning-text hover:bg-warning-text/10
                focus-visible:outline focus-visible:outline-2 focus-visible:outline-warning-text"
                  use:tooltip={{ text: $t('mods.preflight.updateTip'), describe: false }}
                  onclick={() => onUpdate(v)}
                >
                  {$t('mods.preflight.update')}
                </button>
                <button
                  type="button"
                  class="shrink-0 text-xs underline text-warning-text hover:opacity-80
                focus-visible:outline focus-visible:outline-2 focus-visible:outline-warning-text"
                  use:tooltip={{ text: $t('mods.preflight.chooseVersionTip'), describe: false }}
                  onclick={() => onChooseVersion(v)}
                >
                  {$t('mods.preflight.chooseVersion')}
                </button>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}
