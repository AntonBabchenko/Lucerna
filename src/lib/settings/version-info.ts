// Which build this is, in words (Settings → About). Two forms: the line shown
// under the version, in the interface language, and the block "Copy version
// info" puts on the clipboard — English and fixed, because it goes into an
// English issue form.
import type { Translate } from '$lib/i18n';
import type { BuildInfo } from '$lib/ipc/bindings';

/** `v0.25.0-rc.2 · a1b2c3d · windows x86_64`, or `Local build · …`. */
export function buildLine(info: BuildInfo, t: Translate): string {
  const b = info.build;
  const kind =
    b.kind === 'tagged'
      ? b.tag
      : b.kind === 'unrecognised'
        ? t('settings.about.buildUnrecognised', { raw: b.raw })
        : b.kind === 'local'
          ? t('settings.about.buildLocal')
          : t('settings.about.buildDevelopment');
  const parts = [kind];
  if (info.commit) parts.push(info.commit);
  parts.push(`${info.os} ${info.arch}`);
  if (info.fork) parts.push(t('settings.about.buildFork', { repo: info.fork }));
  return parts.join(' · ');
}

/**
 * The paste-ready block. `info` is null when the build info could not be read:
 * the block still says which version the page itself was built with, and says
 * `unknown` rather than leaving a field blank.
 */
export function versionInfoBlock(info: BuildInfo | null, pkgVersion: string): string {
  if (!info) return `Lucerna ${pkgVersion}\nBuild: unknown\nOS: unknown`;
  const b = info.build;
  const build =
    b.kind === 'tagged'
      ? info.commit
        ? `${b.tag} (${info.commit})`
        : b.tag
      : b.kind === 'unrecognised'
        ? `${b.raw} (unrecognised)`
        : b.kind === 'local'
          ? 'local'
          : 'development';
  const lines = [`Lucerna ${info.version}`, `Build: ${build}`];
  if (info.fork) lines.push(`Fork: ${info.fork}`);
  lines.push(`OS: ${info.os} ${info.arch}`);
  return lines.join('\n');
}
