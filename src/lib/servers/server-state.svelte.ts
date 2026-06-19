import {
  commands,
  events,
  type ServerDiagnosis,
  type ServerWithStatus,
  type UploadConfig,
} from '$lib/ipc/bindings';
import { appendCapped, MAX_CONSOLE_LINES } from './console-buffer';

// Single source of truth for own-server runtime state.
// All UI surfaces (sidebar, server list, console panel) read from this store
// so a refresh hits the IPC layer once and every surface reflects it.

let list = $state<ServerWithStatus[]>([]);
let lines = $state<Map<string, string[]>>(new Map());
let diagnoses = $state<Map<string, ServerDiagnosis>>(new Map());
let uploadProgress = $state<Map<string, { done: number; total: number; file: string }>>(new Map());
let initialized = false;

async function refresh(): Promise<void> {
  const res = await commands.serverList();
  if (res.status === 'ok') list = res.data;
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

async function diagnose(id: string): Promise<void> {
  const r = await commands.serverDiagnose(id);
  if (r.status === 'ok') {
    const m = new Map(diagnoses);
    m.set(id, r.data);
    diagnoses = m;
  }
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

export const serverState = {
  get list() {
    return list;
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
  setUploadConfig,
  upload,
  exportZip,
  uploadProgressFor,
  clearUploadProgress,
  rename,
  updateRuntimeConfig,
  remove,
  init,
  running(id: string): boolean {
    return list.find((s) => s.id === id)?.running ?? false;
  },
  get anyRunning() {
    return list.some((s) => s.running);
  },
};
