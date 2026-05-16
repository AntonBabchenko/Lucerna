<script lang="ts">
  import { commands, type CrashReport, type LogFileMeta, type LogSource } from '$lib/ipc/bindings';

  let {
    open = $bindable(false),
    initialPath = null as string | null,
    instanceId = null as string | null,
  }: {
    open: boolean;
    initialPath?: string | null;
    instanceId?: string | null;
  } = $props();

  const CAP_STORAGE_KEY = 'ftl.logs.cap_bytes';
  const CAP_OPTIONS: { label: string; value: number }[] = [
    { label: '1 MB', value: 1 * 1024 * 1024 },
    { label: '5 MB', value: 5 * 1024 * 1024 },
    { label: '25 MB', value: 25 * 1024 * 1024 },
    { label: '100 MB', value: 100 * 1024 * 1024 },
  ];

  function readCapFromStorage(): number {
    if (typeof localStorage === 'undefined') return CAP_OPTIONS[1].value;
    const raw = localStorage.getItem(CAP_STORAGE_KEY);
    const parsed = raw ? Number(raw) : NaN;
    if (!Number.isFinite(parsed) || parsed <= 0) return CAP_OPTIONS[1].value;
    return parsed;
  }

  let files = $state<LogFileMeta[]>([]);
  let listError = $state<string | null>(null);
  let selectedPath = $state<string | null>(null);
  let selectedContent = $state<string>('');
  let contentError = $state<string | null>(null);
  let loadingContent = $state(false);
  let capBytes = $state<number>(readCapFromStorage());
  let search = $state('');

  $effect(() => {
    if (open) {
      void reloadList();
    }
  });

  $effect(() => {
    if (open && initialPath && initialPath !== selectedPath) {
      selectedPath = initialPath;
      void loadContent(initialPath);
    }
  });

  async function reloadList() {
    if (!instanceId) {
      files = [];
      return;
    }
    listError = null;
    const result = await commands.listLogFiles(instanceId);
    if (result.status === 'ok') {
      files = result.data;
    } else {
      listError = JSON.stringify(result.error);
    }
  }

  async function selectFile(path: string) {
    selectedPath = path;
    await loadContent(path);
  }

  async function loadContent(path: string) {
    loadingContent = true;
    contentError = null;
    const result = await commands.readLogFile(path, capBytes);
    loadingContent = false;
    if (result.status === 'ok') {
      selectedContent = result.data;
    } else {
      contentError = JSON.stringify(result.error);
      selectedContent = '';
    }
  }

  function onCapChange(value: number) {
    capBytes = value;
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(CAP_STORAGE_KEY, String(value));
    }
    if (selectedPath) {
      void loadContent(selectedPath);
    }
  }

  function sourceLabel(s: LogSource): string {
    switch (s) {
      case 'game':
        return 'Game logs';
      case 'crash':
        return 'Crash reports';
      case 'launcher':
        return 'Launcher logs';
    }
  }

  function formatBytes(n: number | null | undefined): string {
    if (n == null) return '';
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
    return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
  }

  function formatMtime(ms: number | null | undefined): string {
    if (!ms) return '';
    return new Date(ms).toLocaleString();
  }

  let groupedFiles = $derived(
    (['game', 'crash', 'launcher'] as LogSource[]).map((src) => ({
      source: src,
      label: sourceLabel(src),
      items: files.filter((f) => f.source === src),
    })),
  );

  let selectedMeta = $derived(files.find((f) => f.path === selectedPath) ?? null);
  // For plain files: meta.size_bytes (raw) > cap reliably implies truncation.
  // For .gz files: meta.size_bytes is the COMPRESSED size, which is almost
  // always < cap even when the decompressed body would exceed it. As a
  // fallback signal we also flag when the returned content length sits
  // exactly at the cap — that's the .take(cap) boundary in read_gz.
  let isTruncated = $derived(
    !!selectedMeta &&
      selectedContent.length > 0 &&
      ((selectedMeta.size_bytes ?? 0) > capBytes || selectedContent.length === capBytes),
  );

  let contentLines = $derived(selectedContent.split('\n'));

  function escapeHtml(s: string): string {
    return s
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;')
      .replaceAll('"', '&quot;')
      .replaceAll("'", '&#39;');
  }

  function escapeRegExp(s: string): string {
    return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  }

  function highlight(line: string): string {
    if (!search) return escapeHtml(line);
    const escapedLine = escapeHtml(line);
    const escapedNeedle = escapeHtml(search);
    const re = new RegExp(escapeRegExp(escapedNeedle), 'gi');
    return escapedLine.replace(re, (match) => `<mark>${match}</mark>`);
  }
</script>

{#if open}
  <div
    class="fixed inset-0 z-40 bg-black/30"
    role="button"
    tabindex="-1"
    onclick={() => (open = false)}
    onkeydown={(e) => {
      if (e.key === 'Escape') open = false;
    }}
  ></div>
  <aside class="fixed inset-y-0 right-0 z-50 w-[min(1100px,95vw)] bg-white shadow-xl flex flex-col">
    <header class="flex items-center justify-between px-4 py-2 border-b">
      <h2 class="text-sm font-semibold">Logs</h2>
      <div class="flex items-center gap-3">
        <label class="text-xs flex items-center gap-1">
          Read cap:
          <select
            class="border rounded px-1 py-0.5 text-xs"
            value={capBytes}
            onchange={(e) => onCapChange(Number((e.currentTarget as HTMLSelectElement).value))}
          >
            {#each CAP_OPTIONS as opt}
              <option value={opt.value}>{opt.label}</option>
            {/each}
          </select>
        </label>
        <button
          class="text-xs border rounded px-2 py-0.5 hover:bg-neutral-100"
          onclick={() => void reloadList()}
        >
          Reload
        </button>
        <button
          class="text-xs border rounded px-2 py-0.5 hover:bg-neutral-100"
          onclick={() => (open = false)}
        >
          Close
        </button>
      </div>
    </header>

    <div class="flex-1 flex overflow-hidden">
      <nav class="w-72 border-r overflow-y-auto text-sm">
        {#if listError}
          <p class="p-3 text-red-700 text-xs">Could not list logs: {listError}</p>
        {:else if files.length === 0}
          <p class="p-3 text-neutral-500 text-xs">No log files yet. Launch Minecraft once.</p>
        {:else}
          {#each groupedFiles as group}
            {#if group.items.length > 0}
              <h3 class="px-3 pt-2 pb-1 text-xs font-semibold uppercase text-neutral-500">
                {group.label}
              </h3>
              <ul>
                {#each group.items as f}
                  <li>
                    <button
                      class="w-full text-left px-3 py-1 hover:bg-neutral-100 {selectedPath ===
                      f.path
                        ? 'bg-blue-50'
                        : ''}"
                      onclick={() => void selectFile(f.path)}
                    >
                      <div class="font-mono text-xs truncate">{f.name}</div>
                      <div class="text-[10px] text-neutral-500">
                        {formatBytes(f.size_bytes)} · {formatMtime(f.modified_unix_ms)}
                      </div>
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
          {/each}
        {/if}
      </nav>

      <section class="flex-1 flex flex-col">
        <div class="px-3 py-2 border-b flex items-center gap-2">
          <input
            class="flex-1 border rounded px-2 py-1 text-xs"
            placeholder="Find in file (case-insensitive)…"
            bind:value={search}
            disabled={!selectedPath}
          />
        </div>
        {#if loadingContent}
          <p class="p-4 text-sm text-neutral-500">Reading…</p>
        {:else if contentError}
          <p class="p-4 text-sm text-red-700">{contentError}</p>
        {:else if !selectedPath}
          <p class="p-4 text-sm text-neutral-500">Select a file on the left to read it.</p>
        {:else}
          {#if isTruncated}
            <div class="px-3 py-1 bg-yellow-50 text-yellow-800 text-xs border-b">
              Truncated — showing last {formatBytes(capBytes)}. Raise cap to see more.
            </div>
          {/if}
          <div class="flex-1 overflow-auto font-mono text-xs leading-tight bg-neutral-50">
            <pre
              class="px-3 py-2 whitespace-pre-wrap break-words">{#each contentLines as line, i}<span
                  class="text-neutral-400 select-none"
                  >{(i + 1).toString().padStart(6, ' ')}: </span>{@html highlight(
                  line,
                )}{'\n'}{/each}</pre>
          </div>
        {/if}
      </section>
    </div>
  </aside>
{/if}
