// Words for a build the platform does not list for this instance: what
// differs, the confirmation's one row, and the same facts read back out of
// the backend's typed refusal. Pure — no IPC, no stores; the translator is
// passed in, so the modal (tags off a `ModVersion`) and the views (fields off
// an error) produce the SAME sentence from the same function.
import type { Translate } from '$lib/i18n';
import { displayLoader } from '$lib/instances/loader-display';
import type { Error as IpcError, LoaderKind } from '$lib/ipc/bindings';

export type OffPlatformFacts = {
  versionMc: string[];
  versionLoaders: LoaderKind[];
  instanceMc: string;
  instanceLoader: LoaderKind;
};

/** One row of `CompatWarningDialog` (`filename` is its display label + key). */
export type OffPlatformRow = { filename: string; reason: string };

/** What differs, in words — «built for», not «looks like»: platform tags are
 *  known, not guessed. An axis is named only when the build carries tags on it
 *  AND they leave the instance out; when neither axis can be named (the tags
 *  fit but the platform's own filtered listing still leaves the build out, or
 *  the build carries no tags) it claims only what is known. */
export function offPlatformReason(facts: OffPlatformFacts, tr: Translate): string {
  const parts: string[] = [];
  if (facts.versionMc.length > 0 && !facts.versionMc.includes(facts.instanceMc)) {
    parts.push(
      tr('mods.detail.offPlatformMc', {
        versions: facts.versionMc.join(', '),
        instance: facts.instanceMc,
      }),
    );
  }
  if (facts.versionLoaders.length > 0 && !facts.versionLoaders.includes(facts.instanceLoader)) {
    parts.push(
      tr('mods.detail.offPlatformLoader', {
        loaders: facts.versionLoaders.map(displayLoader).join(', '),
        instance: displayLoader(facts.instanceLoader),
      }),
    );
  }
  return parts.length > 0 ? parts.join('; ') : tr('mods.browse.mismatchDefault');
}

/** The build's name and version number — without saying the number twice when
 *  the platform's release title already contains it. */
export function offPlatformLabel(name: string, versionNumber: string): string {
  return name.includes(versionNumber) ? name : `${name} ${versionNumber}`;
}

export function offPlatformRows(
  label: string,
  facts: OffPlatformFacts,
  tr: Translate,
): OffPlatformRow[] {
  return [{ filename: label, reason: offPlatformReason(facts, tr) }];
}

/** The refusal's own fields as facts; `null` for every other error, so a
 *  caller can write `const facts = offPlatformFactsOfError(err); if (facts) …`. */
export function offPlatformFactsOfError(err: IpcError): OffPlatformFacts | null {
  if (err.kind !== 'mod_version_not_for_instance') return null;
  return {
    versionMc: err.version_mc,
    versionLoaders: err.version_loaders,
    instanceMc: err.instance_mc,
    instanceLoader: err.instance_loader,
  };
}
