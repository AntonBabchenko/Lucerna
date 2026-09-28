import type { Translate } from '$lib/i18n';
import type { DepViolation } from '$lib/ipc/bindings';
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
