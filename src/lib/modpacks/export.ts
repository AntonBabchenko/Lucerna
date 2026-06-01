import type { ExportModInfo, ModpackFormat } from '$lib/ipc/bindings';

export type ExportModeUi = 'lightweight' | 'full';

/** kebab-case the name, append version, choose the extension by format. */
export function defaultExportFilename(
  instanceName: string,
  version: string,
  format: ModpackFormat,
): string {
  const slug =
    instanceName
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'modpack';
  const ext = format === 'modrinth' ? 'mrpack' : 'zip';
  return `${slug}-${version}.${ext}`;
}

/**
 * Mods that cannot be referenced in the chosen format+mode and therefore
 * appear in the bundle/skip checklist. Mirrors the Rust `classify`.
 */
export function unresolvableMods(
  mods: ExportModInfo[],
  format: ModpackFormat,
  mode: ExportModeUi,
): ExportModInfo[] {
  if (mode === 'full') return [...mods];
  return mods.filter((m) => {
    if (!m.has_ids) return true;
    if (format === 'modrinth') {
      return m.source !== 'modrinth' && m.source !== 'curseforge';
    }
    // curseforge zip
    return m.source !== 'curseforge';
  });
}
