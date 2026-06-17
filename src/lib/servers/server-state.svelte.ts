import { commands, events, type ServerWithStatus } from '$lib/ipc/bindings';
import { appendCapped, MAX_CONSOLE_LINES } from './console-buffer';

// Single source of truth for own-server runtime state.
// All UI surfaces (sidebar, server list, console panel) read from this store
// so a refresh hits the IPC layer once and every surface reflects it.

let list = $state<ServerWithStatus[]>([]);
let lines = $state<Map<string, string[]>>(new Map());
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

function init(): void {
  if (initialized) return;
  initialized = true;

  void events.serverLogLine.listen((e) => pushLine(e.payload.server_id, e.payload.line));
  void events.serverSpawned.listen(() => void refresh());
  void events.serverExited.listen(() => void refresh());
}

export const serverState = {
  get list() {
    return list;
  },
  lines: lineFor,
  refresh,
  clearLines,
  init,
  running(id: string): boolean {
    return list.find((s) => s.id === id)?.running ?? false;
  },
};
