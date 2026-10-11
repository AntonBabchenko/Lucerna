<script lang="ts">
  // The Java an instance runs on (spec 2026-10-11-custom-java §6.1). Self-
  // contained: it owns its IPC, its errors and its saved mark, so its host
  // mounts it in one line. The backend refuses a Java that cannot run the
  // instance and saves nothing, so the field never shows a value it could not
  // launch with.
  import { open as openFile } from '@tauri-apps/plugin-dialog';
  import { get } from 'svelte/store';
  import { t, type Translate } from '$lib/i18n';
  import {
    commands,
    type DetectedJava,
    type Error as IpcError,
    type InstanceWithStatus,
    type JavaChoice,
    type JavaReport,
  } from '$lib/ipc/bindings';
  import { formatError, javaUnusableReasonText } from '$lib/ipc/format-error';
  import Icon from '$lib/ui/icons/Icon.svelte';
  import Select, { type SelectOption } from '$lib/ui/Select.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';

  let {
    instanceId,
    value,
    mcVersion,
    loader,
    loaderVersion,
    ready,
    onChanged,
  }: {
    instanceId: string;
    value: JavaChoice | null;
    /** Reload keys: the Java an instance needs changes with its version and
     *  loader, and is unknown until it is installed. */
    mcVersion: string;
    loader: string;
    loaderVersion: string | null;
    ready: boolean;
    onChanged: (instance: InstanceWithStatus) => void;
  } = $props();

  const BUNDLED = '__lucerna_java__';
  const uid = Math.random().toString(36).slice(2);

  let detected = $state<DetectedJava[] | null>(null);
  let detectFailed = $state(false);
  let required = $state<number | null>(null);
  let report = $state<JavaReport | null>(null);
  let refusal = $state<string | null>(null);
  let busy = $state(false);
  let saved = $state(false);
  // Every load and every pick takes a number; only the latest one's answer is
  // applied, so a slow probe never overwrites a newer choice.
  let seq = 0;

  const savedPath = $derived(value?.kind === 'custom' ? value.path : null);

  function reasonOf(e: IpcError): string {
    return e.kind === 'java_unusable' ? javaUnusableReasonText(e.reason) : formatError(e);
  }

  // A rejected IPC promise is a bridge failure, not a backend answer
  // (`typedError` in the bindings rethrows those); say what it was, as
  // LoaderPicker does.
  function thrownText(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
  }

  $effect(() => {
    // Tracked reload keys; everything after the first await is untracked.
    const key = [instanceId, mcVersion, loader, loaderVersion, ready] as const;
    void load(key[0]);
  });

  async function load(id: string) {
    const mine = ++seq;
    refusal = null;
    report = null;
    saved = false;
    try {
      const req = await commands.instanceJavaRequirement(id);
      if (mine !== seq) return;
      if (detected === null) void detect(false);
      if (req.status === 'error') {
        // `null` from the backend means "not installed yet"; a failed read is
        // not that, so it is said as what it is.
        required = null;
        refusal = formatError(req.error);
        return;
      }
      required = req.data;
      const path = savedPath;
      if (path === null) return;
      const r = await commands.javaCheck(path, required);
      if (mine !== seq) return;
      if (r.status === 'ok') report = r.data;
      else refusal = reasonOf(r.error);
    } catch (e) {
      if (mine === seq) refusal = thrownText(e);
    }
  }

  async function detect(refresh: boolean) {
    detectFailed = false;
    if (refresh) detected = null;
    try {
      const r = await commands.javaDetect(refresh);
      if (r.status === 'ok') {
        detected = r.data;
        return;
      }
    } catch {
      // Falls through: either way the list could not be read.
    }
    detected = [];
    detectFailed = true;
  }

  // Only the major is judged from the list: a `release` file's OS_ARCH
  // spellings vary, and the probe refuses a wrong architecture on pick.
  function tooOld(j: DetectedJava): boolean {
    return required !== null && j.major !== null && j.major < required;
  }

  function labelOf(j: DetectedJava, tr: Translate): string {
    const base =
      j.version === null
        ? tr('instance.manage.java.detectedUnknown')
        : j.vendor
          ? tr('instance.manage.java.detected', { version: j.version, vendor: j.vendor })
          : tr('instance.manage.java.detectedNoVendor', { version: j.version });
    // The home folder's name is what tells two Javas of one version apart.
    const dir = j.home.split(/[\\/]/).filter(Boolean).pop() ?? j.home;
    const label = tr('instance.manage.java.withDir', { label: base, dir });
    return tooOld(j) ? tr('instance.manage.java.tooOldOption', { label }) : label;
  }

  const options = $derived.by((): SelectOption[] => {
    const tr = $t;
    const list: SelectOption[] = [{ value: BUNDLED, label: tr('instance.manage.java.bundled') }];
    const sorted = [...(detected ?? [])].sort(
      (a, b) => Number(tooOld(a)) - Number(tooOld(b)) || (b.major ?? 0) - (a.major ?? 0),
    );
    for (const j of sorted) {
      list.push({ value: j.path, label: labelOf(j, tr), disabled: tooOld(j) });
    }
    if (savedPath !== null && !list.some((o) => o.value === savedPath)) {
      list.push({ value: savedPath, label: tr('instance.manage.java.saved') });
    }
    return list;
  });

  async function commit(choice: JavaChoice) {
    const mine = ++seq;
    busy = true;
    refusal = null;
    saved = false;
    try {
      const r = await commands.setInstanceJava(instanceId, choice);
      if (mine !== seq) return;
      if (r.status === 'ok') {
        report = r.data.report;
        saved = true;
        onChanged(r.data.instance);
      } else {
        refusal = reasonOf(r.error);
      }
    } catch (e) {
      if (mine === seq) refusal = thrownText(e);
    } finally {
      if (mine === seq) busy = false;
    }
  }

  function onPick(v: string | number) {
    if (v === BUNDLED) void commit({ kind: 'bundled' });
    else void commit({ kind: 'custom', path: String(v) });
  }

  // The dialog's filter is a convenience; the backend refuses a non-Java
  // either way. Filters take extensions only.
  function onWindows(): boolean {
    return typeof navigator !== 'undefined' && /Windows/i.test(navigator.userAgent);
  }

  async function browse() {
    const picked = await openFile({
      multiple: false,
      directory: false,
      filters: onWindows()
        ? [{ name: get(t)('instance.manage.java.fileFilter'), extensions: ['exe'] }]
        : undefined,
    });
    if (typeof picked === 'string') await commit({ kind: 'custom', path: picked });
  }

  const status = $derived.by((): { tone: 'success' | 'warning' | 'info'; text: string } | null => {
    const tr = $t;
    if (savedPath === null) return { tone: 'info', text: tr('instance.manage.java.statusBundled') };
    if (report === null) return null;
    const v = report.verdict;
    switch (v.kind) {
      case 'matches':
        return {
          tone: 'success',
          text: tr('instance.manage.java.statusMatches', { version: report.version }),
        };
      case 'newer':
        return {
          tone: 'warning',
          text:
            v.required === 8
              ? tr('instance.manage.java.statusNewerLegacy', { found: v.found })
              : tr('instance.manage.java.statusNewer', {
                  found: v.found,
                  required: v.required,
                }),
        };
      case 'requirement_unknown':
        return { tone: 'warning', text: tr('instance.manage.java.statusUnknown') };
    }
  });

  const message = $derived(
    refusal ?? (busy ? $t('instance.manage.java.checking') : (status?.text ?? null)),
  );
  const tone = $derived(refusal ? 'danger' : busy ? 'info' : (status?.tone ?? 'info'));
</script>

<div class="mt-3" data-testid="java-path-field">
  <div class="mb-1 flex items-center justify-between">
    <span id="java-label-{uid}" class="text-xs text-secondary">
      {$t('instance.manage.java.label')}
    </span>
    {#if saved}
      <span class="text-xs text-success">{$t('instance.manage.java.savedMark')}</span>
    {/if}
  </div>
  <div class="flex items-center gap-2">
    <Select
      class="min-w-0 flex-1"
      value={savedPath ?? BUNDLED}
      {options}
      onChange={onPick}
      disabled={busy}
      ariaLabel={$t('instance.manage.java.label')}
      describedby="java-status-{uid}"
      dataTestid="java-select"
    />
    <button type="button" class="btn-secondary btn-sm" disabled={busy} onclick={browse}>
      {$t('instance.manage.java.browse')}
    </button>
  </div>
  <div class="mt-1 flex items-center justify-between gap-2">
    {#if detected === null}
      <span class="text-xs text-placeholder">{$t('instance.manage.java.searching')}</span>
    {:else if detectFailed}
      <span class="text-xs text-secondary">{$t('instance.manage.java.detectFailed')}</span>
    {:else if detected.length === 0}
      <span class="text-xs text-secondary">{$t('instance.manage.java.noneFound')}</span>
    {:else}
      <span></span>
    {/if}
    <button
      type="button"
      class="btn-secondary btn-sm inline-flex items-center gap-1"
      disabled={busy || detected === null}
      onclick={() => detect(true)}
    >
      <Icon name="refresh" class="icon-spin-hover" />
      {$t('instance.manage.java.refresh')}
    </button>
  </div>
  {#if savedPath !== null}
    <p class="selectable mt-1 break-all font-mono text-xs text-muted">{savedPath}</p>
  {/if}
  <StatusMessage
    id="java-status-{uid}"
    {message}
    {tone}
    class="mt-1 text-xs"
    dataTestid="java-status"
  />
  {#if savedPath !== null && report?.gpu_choice_ignored}
    <p class="mt-1 text-xs text-muted">{$t('instance.manage.java.gpuNote')}</p>
  {/if}
</div>
