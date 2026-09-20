// Words for a build the platform does not list for this instance: what
// differs, the confirmation's one row, and the same facts read back out of
// the backend's typed refusal. Pure — no IPC, no stores; the translator is
// passed in.
//
// RED-ROUND SCAFFOLD: the signatures are final, the bodies are constants.
import type { Translate } from '$lib/i18n';
import type { Error as IpcError, LoaderKind } from '$lib/ipc/bindings';

export type OffPlatformFacts = {
  versionMc: string[];
  versionLoaders: LoaderKind[];
  instanceMc: string;
  instanceLoader: LoaderKind;
};

/** One row of `CompatWarningDialog` (`filename` is its display label + key). */
export type OffPlatformRow = { filename: string; reason: string };

export function offPlatformReason(_facts: OffPlatformFacts, _tr: Translate): string {
  return '';
}

export function offPlatformLabel(_name: string, _versionNumber: string): string {
  return '';
}

export function offPlatformRows(
  _label: string,
  _facts: OffPlatformFacts,
  _tr: Translate,
): OffPlatformRow[] {
  return [];
}

export function offPlatformFactsOfError(_err: IpcError): OffPlatformFacts | null {
  return null;
}
