import {
  type BackupInfo,
  commands,
  events,
  type FirewallState,
  type InstallMissingReport,
  type QuarantineReport,
  type ServerConnectivity,
  type ServerDiagnosis,
  type ServerImportPreview,
  type ServerLogInfo,
  type ServerPublicAddress,
  type ServerWithStatus,
  type UploadConfig,
} from '$lib/ipc/bindings';
import { appendCapped, MAX_CONSOLE_LINES } from './console-buffer';
import { isDiagnosisActionable } from './runtime-extra';

// Single source of truth for own-server runtime state.
// All UI surfaces (sidebar, server list, console panel) read from this store
// so a refresh hits the IPC layer once and every surface reflects it.

let list = $state<ServerWithStatus[]>([]);
let lines = $state<Map<string, string[]>>(new Map());
let diagnoses = $state<Map<string, ServerDiagnosis>>(new Map());
let uploadProgress = $state<Map<string, { done: number; total: number; file: string }>>(new Map());
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
): Promise<{ status: 'ok'; data: null } | { status: 'error'; error: unknown }> {
  return await commands.serverUpload(id, acceptNewHostKey);
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
): Promise<{ ok: boolean; error?: unknown }> {
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
    return { ok: true };
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
  exportZip,
  uploadProgressFor,
  clearUploadProgress,
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
  get anyRunning() {
    return list.some((s) => s.running);
  },
  // True when any server has a one-click repair available (C1 diagnosis_status
  // === 'actionable'). Drives the sidebar wrench badge + the attention item.
  // Reads through the runtime-extra shim until S1's field lands in bindings.
  get anyDiagnosisActionable() {
    return list.some((s) => isDiagnosisActionable(s));
  },
  backupList,
  backupCreate,
  backupRestore,
  backupDelete,
};
