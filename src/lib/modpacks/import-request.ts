import type { ModSource } from '$lib/ipc/bindings';

// Payload that ModpacksTab hands up to the page when the user confirms a
// modpack import from the picker. The PAGE owns the actual import execution
// (the two progress `Channel`s + `ImportProgressView`) so that the modpacks
// modal can be freely closed mid-import — the progress toast lives at the page
// level and survives the modal (and ModpacksTab) unmounting.
export type ModpackImportRequest = {
  // Absolute path to the .mrpack/.zip the picker inspected.
  path: string;
  // SHA1s of the optional files the user chose to include.
  selectedShas: string[];
  // Browse-flow hints stamped onto the new instance (null for drag-drop).
  projectId: string | null;
  source: ModSource | null;
  versionId: string | null;
};
