<script module lang="ts">
  import type { Error as IpcError } from '$lib/ipc/bindings';

  /** The errors this panel answers. */
  export type JavaProblem = Extract<
    IpcError,
    { kind: 'jre_unavailable' } | { kind: 'managed_java_missing' }
  >;

  export function isJavaProblem(e: IpcError | null): e is JavaProblem {
    return e !== null && (e.kind === 'jre_unavailable' || e.kind === 'managed_java_missing');
  }
</script>

<script lang="ts">
  // When Lucerna's Java runtime cannot be had (spec 2026-10-11-custom-java §7):
  // a Java on this computer can run the game instead — for this instance, or
  // for every instance on that Java major — and the install is retried. A
  // Forge/NeoForge install step that needed the runtime is explained, not
  // offered: those steps run on Lucerna's runtime only. A runtime that is just
  // not downloaded yet is downloaded on request, never silently.
  import { t } from '$lib/i18n';
  import { commands, type DetectedJava } from '$lib/ipc/bindings';
  import { formatError } from '$lib/ipc/format-error';
  import { writeGeneralVia } from '$lib/settings/app-settings.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import CloseButton from '$lib/ui/CloseButton.svelte';
  import Select, { type SelectOption } from '$lib/ui/Select.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';
  import { LEGACY_JAVA_MAJOR } from './legacy-java';
  import { pickJavaFile } from './pick-java-file';

  let {
    problem,
    message,
    instanceId,
    followsSettings,
    onJavaSaved,
    onRuntimeReady,
    onProblem,
    onDismiss,
  }: {
    problem: JavaProblem;
    /** The error as the page already words it. */
    message: string;
    instanceId: string;
    /** The instance stores no Java of its own, so a Settings row applies to
     *  it. Pinned to Lucerna's Java or to its own, a row would not help it,
     *  and «every instance on Java N» is not offered. */
    followsSettings: boolean;
    /** A Java was saved for this instance (by id — the user may have switched
     *  meanwhile): the page runs its install again. */
    onJavaSaved: (instanceId: string) => void;
    /** Lucerna's runtime is downloaded: nothing else needs installing. */
    onRuntimeReady: (instanceId: string) => void;
    /** The download failed in a way this panel answers too. */
    onProblem: (e: JavaProblem) => void;
    onDismiss: () => void;
  } = $props();

  /** At most this many unknown-version Javas are probed at once. */
  const PROBE_CONCURRENCY = 4;

  let candidates = $state<DetectedJava[] | null>(null);
  let detectFailed = $state(false);
  let picked = $state<string | null>(null);
  /** A file picked by Browse; probed by the write, like any other choice. */
  let browsed = $state<string | null>(null);
  let busy = $state(false);
  let failure = $state<string | null>(null);

  const required = $derived(problem.kind === 'jre_unavailable' ? problem.major : null);
  const offersJava = $derived(
    problem.kind === 'jre_unavailable' && problem.stage === 'game_runtime',
  );
  // For Java 8 only a Java 8 is offered: Forge for those versions does not start on a newer one,
  // and «every instance on Java 8» would move every such instance following Settings onto it.
  const exact = $derived(required === LEGACY_JAVA_MAJOR);
  const fits = (major: number, need: number) =>
    need === LEGACY_JAVA_MAJOR ? major === need : major >= need;
  // «Every instance on Java N» only for a Java from the offered list: the panel saw its version.
  const pickedIsOffered = $derived((candidates ?? []).some((j) => j.path === picked));

  $effect(() => {
    if (offersJava) void findCandidates();
  });

  async function findCandidates() {
    const need = required;
    if (need === null) return;
    let found: DetectedJava[] = [];
    detectFailed = false;
    try {
      const r = await commands.javaDetect(false);
      if (r.status === 'ok') found = r.data;
      else detectFailed = true;
    } catch {
      // Not "nothing found": the search itself failed, and the panel says so.
      detectFailed = true;
    }
    const known = found.filter((j) => j.major !== null && fits(j.major, need));
    // A Java without a `release` file says nothing about its version; ask it.
    const unknown = found.filter((j) => j.major === null);
    const confirmed: DetectedJava[] = [];
    for (let i = 0; i < unknown.length; i += PROBE_CONCURRENCY) {
      const batch = unknown.slice(i, i + PROBE_CONCURRENCY);
      const checks = await Promise.all(batch.map((j) => probe(j, need)));
      for (const c of checks) if (c) confirmed.push(c);
    }
    // As the Java field ranks them: the major the version names first.
    const rank = (j: DetectedJava) => (j.major === need ? 0 : 1);
    candidates = [...known, ...confirmed].sort(
      (a, b) => rank(a) - rank(b) || (b.major ?? 0) - (a.major ?? 0),
    );
    picked = candidates[0]?.path ?? null;
  }

  async function probe(j: DetectedJava, need: number): Promise<DetectedJava | null> {
    try {
      const r = await commands.javaCheck(j.path, need);
      // The probe accepts a newer Java too; for Java 8 only an exact match is offered.
      return r.status === 'ok' && fits(r.data.major, need)
        ? { ...j, major: r.data.major, version: r.data.version }
        : null;
    } catch {
      return null;
    }
  }

  function homeName(home: string): string {
    return home.split(/[\\/]/).filter(Boolean).pop() ?? home;
  }

  const options = $derived.by((): SelectOption[] => {
    const found = (candidates ?? []).map((j) => ({
      value: j.path,
      label: `Java ${j.version ?? '?'} (${homeName(j.home)})`,
    }));
    const extra =
      browsed !== null && !found.some((o) => o.value === browsed)
        ? [{ value: browsed, label: browsed }]
        : [];
    return [...found, ...extra];
  });

  async function browse() {
    failure = null;
    try {
      const path = await pickJavaFile();
      if (path === null) return;
      browsed = path;
      picked = path;
    } catch (e) {
      failure = e instanceof Error ? e.message : String(e);
    }
  }

  async function useForInstance() {
    const path = picked;
    const id = instanceId;
    if (path === null) return;
    await act(id, async () => {
      const r = await commands.setInstanceJava(id, { kind: 'custom', path });
      return r.status === 'ok' ? null : formatError(r.error);
    });
  }

  async function useForMajor() {
    const path = picked;
    const major = required;
    const id = instanceId;
    if (path === null || major === null) return;
    await act(id, async () => {
      const r = await writeGeneralVia(
        () => commands.setJavaDefault(major, path),
        (d) => d.settings,
      );
      return r.status === 'ok' ? null : formatError(r.error);
    });
  }

  // A write that failed retries nothing: the choice was not saved, so the
  // install would fail the same way.
  async function act(id: string, write: () => Promise<string | null>) {
    busy = true;
    failure = null;
    try {
      const err = await write();
      if (err !== null) {
        failure = err;
        return;
      }
      onJavaSaved(id);
    } catch (e) {
      failure = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function downloadRuntime() {
    const id = instanceId;
    busy = true;
    failure = null;
    try {
      const r = await commands.ensureInstanceJre(id);
      if (r.status === 'ok') onRuntimeReady(id);
      else if (isJavaProblem(r.error)) onProblem(r.error);
      else failure = formatError(r.error);
    } catch (e) {
      failure = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div
  class="flex flex-col gap-2 rounded border border-danger/40 bg-surface p-3"
  data-testid="java-rescue"
>
  <div class="flex items-start justify-between gap-2">
    <p class="text-xs text-danger">{message}</p>
    <CloseButton onClick={onDismiss} ariaLabel={$t('page.overview.dismissError')} />
  </div>

  {#if problem.kind === 'managed_java_missing'}
    <p class="text-xs text-secondary">{$t('java.rescue.missingExplain')}</p>
    <BusyButton
      class="btn-primary btn-sm self-start"
      {busy}
      onclick={() => void downloadRuntime()}
      data-testid="java-rescue-download"
    >
      {$t('java.rescue.download')}
    </BusyButton>
  {:else if !offersJava}
    <!-- The install button on Overview is the retry; this says when it can work. -->
    <p class="text-xs text-secondary" data-testid="java-rescue-installer">
      {$t('java.rescue.installerExplain')}
      {problem.kind === 'jre_unavailable' && problem.reason.kind === 'download'
        ? $t('java.rescue.installerRetry')
        : $t('java.rescue.installerNone')}
    </p>
  {:else if candidates === null}
    <span class="text-xs text-placeholder">{$t('instance.manage.java.searching')}</span>
  {:else}
    {#if options.length === 0}
      <p class="text-xs text-secondary" data-testid="java-rescue-none">
        {detectFailed
          ? $t(exact ? 'java.rescue.searchFailedExact' : 'java.rescue.searchFailed', {
              major: required ?? 0,
            })
          : $t(exact ? 'java.rescue.noneExact' : 'java.rescue.none', { major: required ?? 0 })}
      </p>
    {:else}
      <p class="text-xs text-secondary">
        {$t(exact ? 'java.rescue.offerExact' : 'java.rescue.offer', { major: required ?? 0 })}
      </p>
      <Select
        class="text-sm"
        value={picked}
        {options}
        onChange={(v) => (picked = String(v))}
        disabled={busy}
        ariaLabel={$t('java.rescue.pickLabel')}
        dataTestid="java-rescue-select"
      />
    {/if}
    <div class="flex flex-wrap gap-2">
      {#if picked !== null}
        <BusyButton class="btn-primary btn-sm" {busy} onclick={() => void useForInstance()}>
          {$t('java.rescue.forInstance')}
        </BusyButton>
        {#if followsSettings && pickedIsOffered}
          <BusyButton class="btn-secondary btn-sm" {busy} onclick={() => void useForMajor()}>
            {$t('java.rescue.forMajor', { major: required ?? 0 })}
          </BusyButton>
        {/if}
      {/if}
      <button type="button" class="btn-secondary btn-sm" disabled={busy} onclick={browse}>
        {$t('instance.manage.java.browse')}
      </button>
    </div>
  {/if}
  <StatusMessage message={failure} tone="danger" class="text-xs" />
</div>
