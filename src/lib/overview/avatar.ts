import type { LoaderKind, ModSource } from '$lib/ipc/bindings';

/** Avatar tint key: a loader family, or a modpack source for pack instances. */
export type AvatarTone = LoaderKind | ModSource;

export interface Avatar {
  letter: string;
  tone: AvatarTone;
}

/**
 * Derive a network-free avatar for an instance: the uppercased first
 * codepoint of its name, tinted by source (pack instances) or loader.
 */
export function deriveAvatar(instance: {
  name: string;
  loader: LoaderKind;
  mrpack_source: ModSource | null;
}): Avatar {
  const first = Array.from(instance.name.trimStart())[0];
  const letter = first ? first.toUpperCase() : '?';
  const tone: AvatarTone = instance.mrpack_source ?? instance.loader;
  return { letter, tone };
}
