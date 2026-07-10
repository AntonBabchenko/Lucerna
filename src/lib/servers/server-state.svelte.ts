import {
  type BackupInfo,
  commands,
  events,
  type FirewallState,
  type InstallMissingReport,
  type LastUpload,
  type QuarantineReport,
  type ServerConnectivity,
  type ServerDiagnosis,
  type ServerImportPreview,
  type ServerLogInfo,
  type ServerPublicAddress,
  type ServerWithStatus,
  type UploadConfig,
  type UploadPreflight,
} from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import type { NavStatusKind } from '$lib/layout/nav-status';
import { appendCapped, MAX_CONSOLE_LINES } from './console-buffer';
import { isCrashed, isDiagnosisActionable } from './runtime-extra';

// Single source of truth for own-server runtime state.
// All UI surfaces (sidebar, server list, console panel) read from this store
// so a refresh hits the IPC layer once and every surface reflects it.

// Per-server status, mirroring `serversNavStatus`'s precedence for ONE server so
// each list row can paint its icon exactly like the sidebar aggregate does:
// fixable (a crash with a one-click fix) > crashed (a crash with none) >
// running > idle. Pure (takes the server as an arg) so it can't be a store
// getter; the row derives its NavStatusIcon colour/pulse + StatusBadge from it.
export function serverNavStatus(s: ServerWithStatus): NavStatusKind {
  if (isDiagnosisActionable(s)) return 'fixable';
  if (isCrashed(s)) return 'crashed';
  if (s.running) return 'running';
  return 'idle';
}

let list = $state<ServerWithStatus[]>([]);
let lines = $state<Map<string, string[]>>(new Map());
let diagnoses = $state<Map<string, ServerDiagnosis>>(new Map());
let uploadProgress = $state<Map<string, { done: number; total: number; file: string }>>(new Map());

export type UploadPhase = 'uploading' | 'done' | 'error' | 'cancelled';
export interface UploadState {
  phase: UploadPhase;
  filesDone: number;
  filesTotal: number;
  bytesDone: number;
  bytesTotal: number;
  currentFile: string;
  startedAtMs: number;
  error?: string;
}
let uploads = $state<Map<string, UploadState>>(new Map());

export type ServerAction = 'start' | 'stop' | 'restart';
let actionBusy = $state<Map<string, ServerAction>>(new Map());
let actionErrors = $state<Map<string, unknown>>(new Map());

function setUploadState(id: string, patch: Partial<UploadState>): void {
  const m = new Map(uploads);
  const prev = m.get(id);
  const base: UploadState = prev ?? {
    phase: 'uploading',
    filesDone: 0,
    filesTotal: 0,
    bytesDone: 0,
    bytesTotal: 0,
    currentFile: '',
    startedAtMs: 0,
  };
  m.set(id, { ...base, ...patch });
  uploads = m;
}

let initialized = false;
// List fetch state (#23): the view distinguishes a first-load spinner and an
// error/retry surface from a genuinely empty list, instead of every failure
// silently rendering as "no servers yet". Raw error kept so callers/the view
// format it consistently with the rest of the store.
let listLoading = $state(false);
let listError = $state<unknown>(null);

async function refresh(): Promise<void> {
  listLoading = true;
  listError = null;
  try {
    const res = await commands.serverList();
    if (res.status === 'ok') list = res.data;
    else listError = res.error;
  } finally {
    listLoading = false;
  }
}

function lineFor(id: string): string[] {
  return lines.get(id) ?? [];
}

function pushLine(id: string, line: string): void {
  const next = appendCapped(lines.get(id) ?? [], line, MAX_CONSOLE_LINES);
  // Reassign the Map so Svelte 5 $state reactivity fires.
  const m = new Map(lines);
  m.set(id, next);
  lines = m;
}

function clearLines(id: string): void {
  const m = new Map(lines);
  m.set(id, []);
  lines = m;
}

async function diagnose(id: string): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverDiagnose(id);
  if (r.status === 'ok') {
    const m = new Map(diagnoses);
    m.set(id, r.data);
    diagnoses = m;
    return { ok: true };
  }
  // Surface failure so callers (e.g. the manual Diagnose button) don't claim
  // success when the command errored.
  return { ok: false, error: r.error };
}

function diagnosisFor(id: string): ServerDiagnosis | undefined {
  return diagnoses.get(id);
}

function clearDiagnosis(id: string): void {
  const m = new Map(diagnoses);
  m.delete(id);
  diagnoses = m;
}

async function removeClientMods(
  id: string,
  filenames: string[],
  logSignature: string | null,
): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverRemoveMods(id, filenames, logSignature);
  if (r.status === 'ok') {
    const m = new Map(diagnoses);
    m.delete(id);
    diagnoses = m;
    return { ok: true };
  }
  return { ok: false, error: r.error };
}

/// One-click pre-spawn fixes (class A). Each clears the diagnosis on success;
/// the caller re-runs `diagnose` (or the user retries Start) afterwards.
async function acceptEula(id: string): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverAcceptEula(id);
  if (r.status === 'ok') {
    clearDiagnosis(id);
    return { ok: true };
  }
  return { ok: false, error: r.error };
}

async function stopOrphan(id: string, pid: number): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverStopOrphan(id, pid);
  if (r.status === 'ok') {
    clearDiagnosis(id);
    return { ok: true };
  }
  return { ok: false, error: r.error };
}

function setActionBusy(id: string, a: ServerAction | null): void {
  const m = new Map(actionBusy);
  if (a === null) m.delete(id);
  else m.set(id, a);
  actionBusy = m;
}

function setActionError(id: string, err: unknown | null): void {
  const m = new Map(actionErrors);
  if (err === null) m.delete(id);
  else m.set(id, err);
  actionErrors = m;
}

// One lifecycle implementation for the sidebar Start/Stop and the panel
// Restart, so busy/error/diagnose behavior can't drift between surfaces.
// Only 'start' failures trigger a diagnose — parity with the old
// ServerManageView handlers (stop/restart failures never diagnosed).
async function runLifecycle(id: string, action: ServerAction): Promise<{ ok: boolean }> {
  if (actionBusy.get(id)) return { ok: false }; // one in-flight action per server
  setActionBusy(id, action);
  setActionError(id, null);
  try {
    let res: Awaited<ReturnType<typeof commands.serverStart | typeof commands.serverStop>>;
    try {
      res =
        action === 'start'
          ? await commands.serverStart(id)
          : action === 'stop'
            ? await commands.serverStop(id)
            : await commands.serverRestart(id);
    } catch (e) {
      // A thrown IPC failure (transport error, not a typed Result) must land
      // in actionErrors like any other failure — otherwise the returned
      // promise rejects and a `void`-ing caller swallows it with no
      // user-visible trace. Busy is still cleared by the outer finally.
      setActionError(id, e);
      if (action === 'start') void diagnose(id);
      return { ok: false };
    }
    if (res.status !== 'ok') {
      setActionError(id, res.error);
      if (action === 'start') void diagnose(id);
      return { ok: false };
    }
    await refresh();
    return { ok: true };
  } finally {
    setActionBusy(id, null);
  }
}

async function changePort(id: string, port: number): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverChangePort(id, port);
  if (r.status === 'ok') {
    clearDiagnosis(id);
    return { ok: true };
  }
  return { ok: false, error: r.error };
}

/// Class-B (post-spawn log) fixes. Each clears the diagnosis on success; the
/// caller re-runs `diagnose` (or the user retries Start) afterwards.
async function raiseHeap(id: string, toMb: number): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverRaiseHeap(id, toMb);
  if (r.status === 'ok') {
    clearDiagnosis(id);
    return { ok: true };
  }
  return { ok: false, error: r.error };
}

async function lowerHeap(id: string, toMb: number): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverLowerHeap(id, toMb);
  if (r.status === 'ok') {
    clearDiagnosis(id);
    return { ok: true };
  }
  return { ok: false, error: r.error };
}

async function redownloadJar(id: string): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverRedownloadJar(id);
  if (r.status === 'ok') {
    clearDiagnosis(id);
    return { ok: true };
  }
  return { ok: false, error: r.error };
}

async function disableMods(
  id: string,
  filenames: string[],
  logSignature: string | null,
): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverDisableMods(id, filenames, logSignature);
  if (r.status === 'ok') {
    clearDiagnosis(id);
    return { ok: true };
  }
  return { ok: false, error: r.error };
}

async function installMissingDep(
  id: string,
  modIds: string[],
): Promise<{ ok: boolean; report?: InstallMissingReport; error?: unknown }> {
  // Honest feedback: return the report (installed vs unresolved) and DON'T clear
  // the diagnosis here — the caller decides, so a no-op install can't pretend the
  // problem is solved. The banner re-diagnoses only when something was installed.
  const r = await commands.serverInstallMissingDep(id, modIds);
  if (r.status === 'ok') {
    return { ok: true, report: r.data };
  }
  return { ok: false, error: r.error };
}

/// Proactively set aside client-only mods on an existing server (rename to
/// `*.disabled`). Returns the report; the banner clears + re-diagnoses on success.
async function quarantineClientMods(
  id: string,
): Promise<{ ok: boolean; report?: QuarantineReport; error?: unknown }> {
  const r = await commands.serverQuarantineClientMods(id);
  if (r.status === 'ok') {
    return { ok: true, report: r.data };
  }
  return { ok: false, error: r.error };
}

async function setUploadConfig(
  id: string,
  config: UploadConfig,
  password: string | null,
): Promise<{ status: 'ok'; data: null } | { status: 'error'; error: unknown }> {
  return await commands.serverSetUploadConfig(id, config, password);
}

async function upload(
  id: string,
  acceptNewHostKey: boolean,
  skipWorlds: boolean,
  password?: string | null,
  resume = false,
): Promise<{ status: 'ok'; data: null } | { status: 'error'; error: unknown }> {
  setUploadState(id, {
    phase: 'uploading',
    filesDone: 0,
    filesTotal: 0,
    bytesDone: 0,
    bytesTotal: 0,
    currentFile: '',
    startedAtMs: Date.now(),
    error: undefined,
  });
  let r: { status: 'ok'; data: null } | { status: 'error'; error: unknown };
  try {
    r = await commands.serverUpload(id, acceptNewHostKey, skipWorlds, password ?? null, resume);
  } catch (e) {
    setUploadState(id, { phase: 'error', error: String(e) });
    return { status: 'error', error: e };
  }
  if (r.status === 'ok') {
    setUploadState(id, { phase: 'done' });
  } else {
    const err = r.error as { kind?: string };
    // sftp_host_key_mismatch is handled by the UI (re-trust dialog); treat it as
    // cancelled so no persisted generic error is shown alongside the dialog.
    const silentKinds = ['upload_cancelled', 'sftp_host_key_mismatch'];
    const phase: UploadPhase = silentKinds.includes(err?.kind ?? '') ? 'cancelled' : 'error';
    setUploadState(id, {
      phase,
      error:
        phase === 'error' ? formatError(r.error as Parameters<typeof formatError>[0]) : undefined,
    });
  }
  return r;
}

async function cancelUpload(
  id: string,
): Promise<{ status: 'ok'; data: null } | { status: 'error'; error: unknown }> {
  return await commands.serverCancelUpload(id);
}

interface UploadResumeInfo {
  resumable: boolean;
  filesTotal: number;
  filesDone: number;
  bytesTotal: number;
}

/// Ask the backend whether a resumable upload exists for the server's current
/// target. Returns null when none (or on IPC error) so callers can simply
/// `if (info)` to decide whether to show the Continue affordance.
async function uploadResumeState(id: string): Promise<UploadResumeInfo | null> {
  const r = await commands.serverUploadResumeState(id);
  if (r.status !== 'ok' || !r.data.resumable) return null;
  return {
    resumable: true,
    filesTotal: r.data.files_total,
    filesDone: r.data.files_done,
    bytesTotal: r.data.bytes_total ?? 0,
  };
}

/// Size/free-space preflight (#K): total bytes of the selected set (honouring
/// `skipWorlds`) plus remote free space when the host advertises `statvfs`.
/// Returns null on IPC error so the tab degrades to a "couldn't check" line.
async function uploadPreflight(
  id: string,
  acceptNewHostKey: boolean,
  skipWorlds: boolean,
): Promise<UploadPreflight | null> {
  const r = await commands.serverUploadPreflight(id, acceptNewHostKey, skipWorlds);
  return r.status === 'ok' ? r.data : null;
}

/// Last successful upload (timestamp + target) for a server, read from the
/// already-loaded list (the tab refreshes after an upload, so the line updates
/// without a new event). Null until the first successful upload.
function lastUploadFor(id: string): LastUpload | null {
  return list.find((s) => s.id === id)?.upload?.last_upload ?? null;
}

async function exportZip(
  id: string,
  destPath: string,
): Promise<{ status: 'ok'; data: null } | { status: 'error'; error: unknown }> {
  return await commands.serverExportZip(id, destPath);
}

async function connectivity(id: string): Promise<ServerConnectivity | null> {
  const r = await commands.serverConnectivity(id);
  return r.status === 'ok' ? r.data : null;
}

async function firewallStatus(id: string): Promise<FirewallState | null> {
  const r = await commands.serverFirewallStatus(id);
  return r.status === 'ok' ? r.data : null;
}

/// Public-address snapshot for the Connect view (#6, contract C3): primary LAN
/// address, detected public IP (or null), port, and online-mode. Returns null
/// on IPC error so the view degrades to the LAN-only section.
async function publicAddress(id: string): Promise<ServerPublicAddress | null> {
  const r = await commands.serverPublicAddress(id);
  return r.status === 'ok' ? r.data : null;
}

async function firewallAddRule(id: string): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverFirewallAddRule(id);
  if (r.status === 'ok') return { ok: true };
  return { ok: false, error: r.error };
}

function uploadProgressFor(id: string): { done: number; total: number; file: string } | undefined {
  return uploadProgress.get(id);
}

function clearUploadProgress(id: string): void {
  const m = new Map(uploadProgress);
  m.delete(id);
  uploadProgress = m;
}

function replaceInList(updated: ServerWithStatus): void {
  list = list.map((s) => (s.id === updated.id ? updated : s));
}

async function rename(id: string, name: string): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverRename(id, name);
  if (r.status === 'ok') {
    replaceInList(r.data);
    return { ok: true };
  }
  return { ok: false, error: r.error };
}

async function updateRuntimeConfig(
  id: string,
  maxHeapMb: number,
  extraJvmArgs: string,
): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverUpdateRuntimeConfig(id, maxHeapMb, extraJvmArgs);
  if (r.status === 'ok') {
    replaceInList(r.data);
    return { ok: true };
  }
  return { ok: false, error: r.error };
}

async function remove(id: string): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverDelete(id);
  if (r.status === 'ok') {
    list = list.filter((s) => s.id !== id);
    return { ok: true };
  }
  return { ok: false, error: r.error };
}

async function backupList(
  id: string,
): Promise<{ ok: boolean; list?: BackupInfo[]; error?: unknown }> {
  const r = await commands.serverBackupList(id);
  if (r.status === 'ok') return { ok: true, list: r.data };
  return { ok: false, error: r.error };
}

async function backupCreate(
  id: string,
): Promise<{ ok: boolean; data?: BackupInfo; error?: unknown }> {
  const r = await commands.serverBackupCreate(id);
  if (r.status === 'ok') return { ok: true, data: r.data };
  return { ok: false, error: r.error };
}

async function backupRestore(
  id: string,
  fileName: string,
): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverBackupRestore(id, fileName);
  if (r.status === 'ok') return { ok: true };
  return { ok: false, error: r.error };
}

async function backupDelete(
  id: string,
  fileName: string,
): Promise<{ ok: boolean; error?: unknown }> {
  const r = await commands.serverBackupDelete(id, fileName);
  if (r.status === 'ok') return { ok: true };
  return { ok: false, error: r.error };
}

async function importInspect(
  sourcePath: string,
): Promise<{ ok: boolean; preview?: ServerImportPreview; error?: unknown }> {
  const r = await commands.serverImportInspect(sourcePath);
  if (r.status === 'ok') return { ok: true, preview: r.data };
  return { ok: false, error: r.error };
}

async function importCommit(
  token: string,
  name: string,
  mcVersion: string,
  loader: ServerWithStatus['loader'],
  loaderVersion: string | null,
  maxHeapMb: number,
  eulaAccepted: boolean,
): Promise<{ ok: boolean; server?: ServerWithStatus; error?: unknown }> {
  const r = await commands.serverImportCommit(
    token,
    name,
    mcVersion,
    loader,
    loaderVersion,
    maxHeapMb,
    eulaAccepted,
  );
  if (r.status === 'ok') {
    await refresh();
    // Expose the created server so the import view can auto-select it in the
    // servers panel (mirrors the create wizard's `onDone(createdId)`).
    return { ok: true, server: r.data };
  }
  return { ok: false, error: r.error };
}

async function importCancel(token: string): Promise<void> {
  await commands.serverImportCancel(token);
}

function init(): void {
  if (initialized) return;
  initialized = true;

  void events.serverLogLine.listen((e) => pushLine(e.payload.server_id, e.payload.line));
  void events.serverSpawned.listen((e) => {
    // A retry started — drop any stale pre-spawn banner for this server.
    clearDiagnosis(e.payload.server_id);
    void refresh();
  });
  void events.serverExited.listen((e) => {
    void refresh();
    if (e.payload.code !== 0) {
      void diagnose(e.payload.server_id);
    }
  });
  void events.serverUploadProgress.listen((e) => {
    setUploadState(e.payload.server_id, {
      phase: 'uploading',
      filesDone: e.payload.files_done,
      filesTotal: e.payload.files_total,
      bytesDone: e.payload.bytes_done ?? 0,
      bytesTotal: e.payload.bytes_total ?? 0,
      currentFile: e.payload.current_file,
    });
    const m = new Map(uploadProgress);
    m.set(e.payload.server_id, {
      done: e.payload.files_done,
      total: e.payload.files_total,
      file: e.payload.current_file,
    });
    uploadProgress = m;
  });
}

async function listLogs(id: string): Promise<{ ok: boolean; list?: ServerLogInfo[] }> {
  const r = await commands.serverListLogs(id);
  if (r.status === 'ok') return { ok: true, list: r.data };
  return { ok: false };
}

async function readLog(id: string, fileName: string): Promise<{ ok: boolean; text?: string }> {
  const r = await commands.serverReadLog(id, fileName);
  if (r.status === 'ok') return { ok: true, text: r.data };
  return { ok: false };
}

async function openLogsFolder(id: string): Promise<void> {
  await commands.serverOpenLogsFolder(id);
}

export const serverState = {
  get list() {
    return list;
  },
  get listLoading() {
    return listLoading;
  },
  get listError() {
    return listError;
  },
  lines: lineFor,
  refresh,
  clearLines,
  diagnose,
  diagnosisFor,
  removeClientMods,
  acceptEula,
  stopOrphan,
  changePort,
  raiseHeap,
  lowerHeap,
  redownloadJar,
  disableMods,
  installMissingDep,
  quarantineClientMods,
  setUploadConfig,
  upload,
  uploadResumeState,
  uploadPreflight,
  lastUploadFor,
  cancelUpload,
  exportZip,
  uploadProgressFor,
  clearUploadProgress,
  uploadStateFor(id: string): UploadState | undefined {
    return uploads.get(id);
  },
  isUploading(id: string): boolean {
    return uploads.get(id)?.phase === 'uploading';
  },
  get anyUploading(): boolean {
    return [...uploads.values()].some((u) => u.phase === 'uploading');
  },
  rename,
  updateRuntimeConfig,
  remove,
  connectivity,
  firewallStatus,
  firewallAddRule,
  publicAddress,
  importInspect,
  importCommit,
  importCancel,
  listLogs,
  readLog,
  openLogsFolder,
  init,
  running(id: string): boolean {
    return list.find((s) => s.id === id)?.running ?? false;
  },
  start: (id: string) => runLifecycle(id, 'start'),
  stop: (id: string) => runLifecycle(id, 'stop'),
  restart: (id: string) => runLifecycle(id, 'restart'),
  actionFor(id: string): ServerAction | null {
    return actionBusy.get(id) ?? null;
  },
  actionErrorFor(id: string): unknown {
    return actionErrors.get(id);
  },
  clearActionError(id: string): void {
    setActionError(id, null);
  },
  // True when any server has a one-click repair available (C1 diagnosis_status
  // === 'actionable'). Drives the sidebar wrench badge + the attention item.
  // Reads through the runtime-extra shim until S1's field lands in bindings.
  get anyDiagnosisActionable() {
    return list.some((s) => isDiagnosisActionable(s));
  },
  // Single resolved status for the sidebar "Servers" icon. Precedence:
  // fixable (a crash with a one-click fix) > crashed (a crash with none) >
  // running > idle. The sidebar reads only this so the precedence lives once.
  get serversNavStatus(): 'fixable' | 'crashed' | 'running' | 'idle' {
    if (list.some(isDiagnosisActionable)) return 'fixable';
    if (list.some(isCrashed)) return 'crashed';
    if (list.some((s) => s.running)) return 'running';
    return 'idle';
  },
  backupList,
  backupCreate,
  backupRestore,
  backupDelete,
};
