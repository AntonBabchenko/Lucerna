// The single vocabulary every task surface (registry, bottom strip, expanded
// panel, report modal) speaks. Kept free of runes and IPC so it stays
// importable from plain unit tests and other pure modules.
import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { TaskDetail } from '$lib/ipc/bindings';

export const TASK_KINDS = [
  'game-install',
  'mod-install',
  'mod-update',
  'pack-import',
  'launcher-import',
  'pack-update',
  'clone',
  'verify',
  'repair',
  'server-upload',
  'app-update',
  'data-migration',
] as const;

// `server-core-install` is deliberately absent. Server core provisioning
// downloads through `download_no_emit_with`, whose progress closure is
// literally `|_| {}` (src-tauri/src/network/download.rs), and the
// Forge/NeoForge branch buffers the whole installer run with
// `cmd.output().await` (src-tauri/src/process/mod.rs). There is no progress
// source to adapt in principle, so a task of that kind could never render
// anything — do not "fix" this omission by adding it back.

export type TaskKind = (typeof TASK_KINDS)[number];

export type TaskState = 'queued' | 'running' | 'ok' | 'partial' | 'failed' | 'cancelled';

/**
 * How a task shares the launcher with other tasks:
 * - `serial` — one at a time, reorderable while queued (today's op-queue
 *   behaviour: verify, repair, pack import, launcher import, clone).
 * - `concurrent` — runs alongside anything else (game install, mod
 *   install/update, server upload).
 * - `modal` — owns a blocking dialog; registered for the record, never
 *   cancellable, and may terminate by app restart/exit rather than by
 *   reaching a terminal state (data-root migration, launcher self-update).
 */
export type TaskLane = 'serial' | 'concurrent' | 'modal';

export type TaskProgress = { current: number; total: number; unit: 'files' | 'bytes' };

export type Task = {
  id: string;
  kind: TaskKind;
  scope: { instanceId?: string; serverId?: string };
  title: string;
  phase: string | null;
  progress: TaskProgress | null;
  rate: { bytesPerSec: number; etaSeconds: number | null } | null;
  state: TaskState;
  lane: TaskLane;
  caps: { cancellable: boolean; reorderable: boolean };
  details: TaskDetail[] | null;
  startedAt: number;
  finishedAt: number | null;
};

/** Record, not a lookup function: a new kind without a label must not compile. */
export const KIND_LABEL_KEY: Record<TaskKind, TranslationKey> = {
  'game-install': 'tasks.kind.gameInstall',
  'mod-install': 'tasks.kind.modInstall',
  'mod-update': 'tasks.kind.modUpdate',
  'pack-import': 'tasks.kind.packImport',
  'launcher-import': 'tasks.kind.launcherImport',
  'pack-update': 'tasks.kind.packUpdate',
  clone: 'tasks.kind.clone',
  verify: 'tasks.kind.verify',
  repair: 'tasks.kind.repair',
  'server-upload': 'tasks.kind.serverUpload',
  'app-update': 'tasks.kind.appUpdate',
  'data-migration': 'tasks.kind.dataMigration',
};

export const ORIGIN_LABEL_KEY: Record<TaskDetail['origin'], TranslationKey> = {
  modrinth: 'tasks.origin.modrinth',
  curseforge: 'tasks.origin.curseforge',
  ftb: 'tasks.origin.ftb',
  atlauncher: 'tasks.origin.atlauncher',
  archive: 'tasks.origin.archive',
  local: 'tasks.origin.local',
  game: 'tasks.origin.game',
};
