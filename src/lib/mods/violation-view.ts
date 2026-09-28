import type { Translate } from '$lib/i18n';
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { ChangeDirection, DepViolation, VersionFixPlan } from '$lib/ipc/bindings';
import { formatRange, isSoftRange } from './range-format';

// What each pre-flight ViolationKind reads as, and which repair applies — one
// place, shared by the «What stops the game» panel, the row's reason line, the
// Play gate and `fixAll`. Both switches are exhaustive over the kind, so a new
// kind is a compile error here instead of a silent fall-through (spec §5.1,
// audit A-F2: no Rust match over the kind is exhaustive, so nothing else would
// point at it).

export type ViolationAction = 'enable' | 'install' | 'plan' | 'none';

// `platform_mismatch.dep_id` is `"minecraft"` or the loader's CANONICAL id
// (`preflight.rs` `loader_dep_id`) — a label, never a mod the player knows.
const PLATFORM_LABEL: Record<string, string> = {
  minecraft: 'Minecraft',
  fabricloader: 'Fabric Loader',
  quilt_loader: 'Quilt Loader',
  forge: 'Forge',
  neoforge: 'NeoForge',
};

export function platformLabel(depId: string): string {
  return PLATFORM_LABEL[depId] ?? depId;
}

/**
 * One sentence per violation; `dep` is the dependency's display name. The range
 * is rendered in plain language — raw Maven bracket notation (`(,6.0.9]`) is
 * not something a player can read, and for an incompatibility it would read
 * backwards.
 */
export function violationMessage(t: Translate, v: DepViolation, dep: string): string {
  const dependent = v.dependent_name;
  const installed = v.installed_version ?? '';
  switch (v.kind) {
    case 'missing_required':
      return t('mods.preflight.missing', { dependent, dep });
    case 'required_disabled':
      // Never "not installed": the jar is there, switched off — and the fix is
      // switching it on, not installing a second copy.
      return t('mods.preflight.requiredDisabled', { dependent, dep });
    case 'platform_mismatch':
      return t('mods.preflight.platformMismatch', {
        dependent,
        platform: platformLabel(v.dep_id),
        needed: formatRange(t, v.needed_desc),
        installed,
      });
    case 'incompatible_installed':
      // An incompatibility fires when the installed version is INSIDE the
      // declared range, so a range that constrains nothing fires for every
      // version. Naming versions there would contradict itself — "incompatible
      // with X 1.2 recommended (any version works)" — so drop the range.
      if (isSoftRange(v.needed_desc)) {
        return t('mods.preflight.incompatibleWithAny', { dependent, dep, installed });
      }
      return t('mods.preflight.incompatibleWith', {
        dependent,
        dep,
        needed: formatRange(t, v.needed_desc),
        installed,
      });
    case 'optional_out_of_range':
      return t('mods.preflight.optionalOutOfRange', {
        dependent,
        dep,
        needed: formatRange(t, v.needed_desc),
        installed,
      });
    case 'version_out_of_range':
      return t('mods.preflight.outOfRange', {
        dependent,
        dep,
        needed: formatRange(t, v.needed_desc),
        installed,
      });
  }
}

/**
 * The automatic repair a violation has. `platform_mismatch` has none: it needs
 * another build of the mod itself or the migration plan, never an automatic fix
 * (spec §6.4). A disabled requirement whose jar the report does not name has
 * nothing to switch on.
 */
export function violationAction(v: DepViolation): ViolationAction {
  switch (v.kind) {
    case 'required_disabled':
      return v.provider_sha1 ? 'enable' : 'none';
    case 'missing_required':
      return 'install';
    case 'version_out_of_range':
    case 'optional_out_of_range':
    case 'incompatible_installed':
      return 'plan';
    case 'platform_mismatch':
      return 'none';
  }
}

/** Counted by «Fix all»: a violation with an automatic repair. */
export function isFixable(v: DepViolation): boolean {
  return violationAction(v) !== 'none';
}

/** The planner's answer for one row, keyed by `violationKey` (the Installed tab's «Fix…»). */
export type PlanState =
  | { status: 'loading' }
  | { status: 'ready'; plan: VersionFixPlan }
  | { status: 'failed'; message: string };

/** Which mod a planner fix changes: the one that declared the range, or the one it names. */
export type PlanSide = 'dependent' | 'provider';

export type PlanOffer = {
  side: PlanSide;
  label: string;
  /** The row's default action — never a change that breaks another mod (spec D8). */
  primary: boolean;
  /** Enabled mods a change of the dependency would break: said beside it, applied only on its own click. */
  breaks: string[];
};

// The verb follows the comparator's verdict. `unknown` (a qualifier decides the order) takes the
// neutral one — never a guessed «Update» or «Roll back» (audit A-F5). A Record, so a new direction
// is a compile error here rather than a mislabelled button.
const PROVIDER_CHANGE_KEY: Record<ChangeDirection, TranslationKey> = {
  upgrade: 'mods.preflight.planUpgradeDep',
  downgrade: 'mods.preflight.planDowngradeDep',
  unknown: 'mods.preflight.planSwitchDep',
};

/**
 * The planner's fixes as buttons (spec D8, §6.5). A newer build of the dependent that accepts what
 * is installed comes first: it changes no other mod. Then the change of the dependency, named by
 * its real direction. Only a change that breaks nothing may be the primary action.
 */
export function planOffers(
  t: Translate,
  v: DepViolation,
  plan: VersionFixPlan,
  dep: string,
): PlanOffer[] {
  const out: PlanOffer[] = [];
  if (plan.update_dependent) {
    out.push({
      side: 'dependent',
      primary: true,
      breaks: [],
      label: t('mods.preflight.planUpdateDependent', {
        dependent: v.dependent_name,
        version: plan.update_dependent.version.version_number,
      }),
    });
  }
  const p = plan.change_provider;
  if (p) {
    out.push({
      side: 'provider',
      primary: out.length === 0 && p.breaks.length === 0,
      breaks: [...p.breaks],
      label: t(PROVIDER_CHANGE_KEY[p.direction], { dep, version: p.version.version_number }),
    });
  }
  return out;
}
