<script module lang="ts">
  import type { Error as IpcError, JavaChoice, JavaReport } from '$lib/ipc/bindings';

  /** A host's IPC answer, the shape every generated command returns. */
  export type IpcResult<T> = { status: 'ok'; data: T } | { status: 'error'; error: IpcError };

  /** What a save confirmed: the stored choice and the probe's report. */
  export type JavaSavedValue = { value: JavaChoice | null; report: JavaReport | null };

  /** The «Default» option — `null` choice — and how it reads for a need. */
  export type DefaultOption = {
    labelFor: (required: number | null) => string;
    statusFor: (required: number | null) => string;
  };
</script>

<script lang="ts">
  // Picking a Java (spec 2026-10-11-custom-java §6): Lucerna's runtime, a Java
  // found on this computer, or one picked by Browse — probed by the backend,
  // which refuses one that cannot run the game and saves nothing. The host
  // gives the need (`loadRequired`) and the write (`save`); the field owns the
  // detection, the verdict, the errors and the saved mark. Wrapped by
  // `JavaPathField` (an instance), `JavaDefaultsSection` (Settings, a row per Java major) and
  // `ServerJavaField` (a server).
  import { untrack } from 'svelte';
  import { t, type Translate } from '$lib/i18n';
  import { commands, type DetectedJava } from '$lib/ipc/bindings';
  import { formatError, javaUnusableReasonText } from '$lib/ipc/format-error';
  import Icon from '$lib/ui/icons/Icon.svelte';
  import Select, { type SelectOption } from '$lib/ui/Select.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { pickJavaFile } from './pick-java-file';

  let {
    reloadKey,
    value,
    label,
    loadRequired,
    save,
    defaultOption = null,
    lucernaWrites,
    bundledStatus = null,
    testid = 'java-path-field',
  }: {
    /** Reload when it changes BY VALUE — hosts replace their lists wholesale. */
    reloadKey: string;
    value: JavaChoice | null;
    label: string;
    /** The Java major the version needs; `null` = not known yet. */
    loadRequired: () => Promise<IpcResult<number | null>>;
    save: (choice: JavaChoice | null) => Promise<IpcResult<JavaSavedValue>>;
    defaultOption?: DefaultOption | null;
    /** What the «Lucerna's Java» option stores: an explicit pin where a
     *  «Default» option exists, `null` (no row) in Settings. */
    lucernaWrites: JavaChoice | null;
    /** What «Lucerna's Java» means where the version's own wording does not
     *  fit — a Settings row speaks for every instance on a Java version. */
    bundledStatus?: string | null;
    testid?: string;
  } = $props();

  const LUCERNA = '__lucerna_java__';
  const DEFAULT = '__default__';
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

  // What the backend confirmed it saved, held until the host's list catches
  // up with it. Without it the field would describe the OLD value for the
  // round trip after a save. Reset on every load (the host's reload key),
  // never by a host refresh.
  let confirmed = $state<{ choice: JavaChoice | null } | null>(null);
  const current = $derived(confirmed ? confirmed.choice : value);
  const savedPath = $derived(current?.kind === 'custom' ? current.path : null);

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
    void reloadKey;
    untrack(() => void load());
  });

  async function load() {
    const mine = ++seq;
    confirmed = null;
    refusal = null;
    report = null;
    saved = false;
    try {
      const req = await loadRequired();
      if (mine !== seq) return;
      if (detected === null) void detect(false);
      if (req.status === 'error') {
        // `null` means "not known yet"; a failed read is not that, so it is
        // said as what it is.
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
    const withDir = tr('instance.manage.java.withDir', { label: base, dir });
    return tooOld(j) ? tr('instance.manage.java.tooOldOption', { label: withDir }) : withDir;
  }

  const selected = $derived(
    savedPath ?? (current?.kind === 'bundled' ? LUCERNA : defaultOption ? DEFAULT : LUCERNA),
  );

  const options = $derived.by((): SelectOption[] => {
    const tr = $t;
    const list: SelectOption[] = [];
    if (defaultOption) list.push({ value: DEFAULT, label: defaultOption.labelFor(required) });
    list.push({ value: LUCERNA, label: tr('instance.manage.java.bundled') });
    // The Java the version names first, then the newer ones newest first,
    // then the ones too old for it.
    const rank = (j: DetectedJava) =>
      tooOld(j) ? 2 : required !== null && j.major === required ? 0 : 1;
    const sorted = [...(detected ?? [])].sort(
      (a, b) => rank(a) - rank(b) || (b.major ?? 0) - (a.major ?? 0),
    );
    for (const j of sorted) {
      list.push({ value: j.path, label: labelOf(j, tr), disabled: tooOld(j) });
    }
    if (savedPath !== null && !list.some((o) => o.value === savedPath)) {
      list.push({ value: savedPath, label: tr('instance.manage.java.saved') });
    }
    return list;
  });

  async function commit(choice: JavaChoice | null) {
    const mine = ++seq;
    busy = true;
    refusal = null;
    saved = false;
    try {
      const r = await save(choice);
      if (r.status === 'ok') {
        if (mine === seq) {
          confirmed = { choice: r.data.value };
          report = r.data.report;
          saved = true;
        }
      } else if (mine === seq) {
        refusal = reasonOf(r.error);
      }
    } catch (e) {
      if (mine === seq) refusal = thrownText(e);
    } finally {
      // Unconditional: the controls are disabled while busy, so no other
      // commit can be in flight, and a load that superseded this one must
      // not leave the field locked.
      busy = false;
    }
  }

  function onPick(v: string | number) {
    if (v === DEFAULT) void commit(null);
    else if (v === LUCERNA) void commit(lucernaWrites);
    else void commit({ kind: 'custom', path: String(v) });
  }

  async function browse() {
    let picked: string | null;
    try {
      picked = await pickJavaFile();
    } catch (e) {
      refusal = thrownText(e);
      return;
    }
    if (picked !== null) await commit({ kind: 'custom', path: picked });
  }

  const status = $derived.by((): { tone: 'success' | 'warning' | 'info'; text: string } | null => {
    const tr = $t;
    if (savedPath === null) {
      if (current === null && defaultOption) {
        return { tone: 'info', text: defaultOption.statusFor(required) };
      }
      return { tone: 'info', text: bundledStatus ?? tr('instance.manage.java.statusBundled') };
    }
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

<div class="mt-3" data-testid={testid}>
  <div class="mb-1 flex items-center justify-between">
    <span id="java-label-{uid}" class="text-xs text-secondary">{label}</span>
    {#if saved}
      <span class="text-xs text-success">{$t('instance.manage.java.savedMark')}</span>
    {/if}
  </div>
  <div class="flex items-center gap-2">
    <Select
      class="min-w-0 flex-1"
      value={selected}
      {options}
      onChange={onPick}
      disabled={busy}
      ariaLabel={label}
      describedby="java-status-{uid}"
      dataTestid="{testid}-select"
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
    dataTestid="{testid}-status"
  />
  {#if savedPath !== null && report?.gpu_choice_ignored}
    <p class="mt-1 text-xs text-muted">{$t('instance.manage.java.gpuNote')}</p>
  {/if}
</div>
