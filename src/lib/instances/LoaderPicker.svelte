<script lang="ts">
  import { untrack } from 'svelte';
  import { get } from 'svelte/store';
  import {
    commands,
    type LoaderKind,
    type LoaderVersion,
    type Error as IpcError,
  } from '$lib/ipc/bindings';
  import { displayLoader } from '$lib/instances/loader-display';
  import { resolveLoaderVersion } from '$lib/instances/loader-version';
  import { formatError } from '$lib/ipc/format-error';
  import { t } from '$lib/i18n';
  import Select from '$lib/ui/Select.svelte';
  import Spinner from '$lib/ui/Spinner.svelte';
  import StatusMessage from '$lib/ui/StatusMessage.svelte';

  function formatLoaderError(e: IpcError): string {
    // Picker keeps shorter wording for the 3 variants it surfaces most
    // often; everything else delegates so no raw JSON if e.g. a loader
    // cache IO error reaches the picker.
    if (e.kind === 'loader_unavailable')
      return get(t)('instance.loader.errorUnavailable', {
        loader: displayLoader(e.loader as LoaderKind),
        mc: e.mc_version,
      });
    if (e.kind === 'network')
      return get(t)('instance.loader.errorNetwork', { url: e.url, details: e.details });
    if (e.kind === 'forge_promotions_unavailable')
      return get(t)('instance.loader.errorForgePromotions', { flavor: e.flavor });
    return formatError(e);
  }

  const LOADER_KINDS: LoaderKind[] = ['vanilla', 'fabric', 'quilt', 'forge', 'neoforge'];
  // Unique per instance so two LoaderPickers on a page never collide on the
  // group-label or version-select ids.
  const uid = crypto.randomUUID();
  const groupLabelId = `loader-group-${uid}`;
  const versionSelectId = `loader-version-${uid}`;

  let {
    mc,
    loader = $bindable<LoaderKind>(),
    loaderVersion = $bindable<string | null>(),
    onchange,
    disabled = false,
  }: {
    mc: string;
    loader: LoaderKind;
    loaderVersion: string | null;
    // A COMMIT REQUEST, answered with a verdict. `false` — the parent refused
    // the change or failed to write it — sends the picker back to showing the
    // props. The return is mandatory so a consumer cannot forget to answer.
    // Leave `onchange` out altogether (the create form, which `bind:`s) and the
    // picker edits the bound draft directly instead.
    onchange?: (loader: LoaderKind, version: string | null) => boolean | Promise<boolean>;
    disabled?: boolean;
  } = $props();

  let versions = $state<LoaderVersion[]>([]);
  let error = $state<string | null>(null);
  let isLoadingVersions = $state(false);

  // COMMIT MODE ONLY. With `onchange` the props are the parent's committed
  // truth and are never assigned here: a one-way prop the child writes keeps a
  // local override that only a CHANGED parent value replaces, so a refused
  // change used to stay on screen for good. An uncommitted choice lives here
  // instead and is dropped on refusal.
  //   draft     — clicked; nothing sent yet (list pending, or it failed)
  //   requested — sent; verdict outstanding
  //   accepted  — written; waiting for the props to catch up
  type View = {
    kind: LoaderKind;
    version: string | null;
    phase: 'draft' | 'requested' | 'accepted';
  };
  let view = $state<View | null>(null);
  const shownLoader = $derived(view?.kind ?? loader);
  const shownVersion = $derived(view ? view.version : loaderVersion);

  // Non-reactive bookkeeping — read and written in handlers and after awaits.
  let pickSeq = 0; // bumped per user action; a verdict for an older one is moot
  let loadSeq = 0; // bumped per list fetch; a superseded list never lands
  let outstanding = 0; // requests whose verdict has not arrived
  // A request was accepted and the props have not changed since: the committed
  // pair may already differ from the props, so "back to the props" is no
  // longer free — it needs a real request (last click wins).
  let writtenSinceProps = false;

  function failList(message: string) {
    versions = [];
    error = message;
    // "Could not list builds" is not "no version needed". Bind mode: the draft
    // loses its version, which keeps Create blocked. Commit mode: there is no
    // valid pair to write, so nothing is requested.
    if (!onchange && loaderVersion !== null) loaderVersion = null;
  }

  function requestCommit(k: LoaderKind, v: string | null) {
    if (!onchange) return;
    const seq = pickSeq;
    view = { kind: k, version: v, phase: 'requested' };
    outstanding += 1;
    const settle = (accepted: boolean) => {
      outstanding -= 1;
      const matchesProps = k === loader && v === loaderVersion;
      if (accepted) writtenSinceProps = !matchesProps;
      // A verdict for a pick the user already clicked away from says nothing
      // about what is on screen now.
      if (seq !== pickSeq) return;
      if (!accepted || matchesProps) {
        // Refused, failed or unanswerable: show what the parent holds. (An
        // accepted pair the props already carry needs no view either.)
        view = null;
      } else if (view) {
        // Kept until the props catch up, so the picker does not flash the old
        // value while the parent refetches.
        view = { ...view, phase: 'accepted' };
      }
    };
    let verdict: boolean | Promise<boolean>;
    try {
      verdict = onchange(k, v);
    } catch {
      // A handler that throws did not commit anything.
      settle(false);
      return;
    }
    Promise.resolve(verdict).then(
      (ok) => settle(ok === true),
      () => settle(false),
    );
  }

  // The parent's truth changed → it wins over any local view. Compared by
  // VALUE, not by this effect re-running: a parent that re-derives an equal
  // pair (a refreshed instance list) must not drop a view.
  let seenProps = false;
  let lastLoader: LoaderKind | undefined;
  let lastVersion: string | null | undefined;
  $effect(() => {
    const l = loader;
    const v = loaderVersion;
    if (seenProps && (l !== lastLoader || v !== lastVersion)) {
      writtenSinceProps = false;
      untrack(() => {
        if (onchange && view) view = null;
      });
    }
    seenProps = true;
    lastLoader = l;
    lastVersion = v;
  });

  // Refetch whenever (mc, shown loader) change, then resolve the version. One
  // path for a click, a mount, an MC change and a parent-driven loader change:
  // a click clears the version first, so "reset to the new ecosystem's stable"
  // falls out of the ordinary resolve, while a version the parent passed is
  // kept whenever it is a real build for this MC. (A pack restore that writes
  // the pack's loader back is therefore never mistaken for the user switching
  // ecosystems — only a click creates a view.)
  $effect(() => {
    const m = mc;
    const k = shownLoader;
    loadSeq += 1;
    const seq = loadSeq;
    if (k === 'vanilla' || !m) {
      versions = [];
      // Clear any stale error from a previous (mc, loader) attempt —
      // without this, switching to vanilla (or wiping MC via openCreate)
      // leaves "Quilt does not support Minecraft 26.1.2" hanging below
      // the loader row from a prior failed pick.
      error = null;
      isLoadingVersions = false;
      untrack(() => resolveWithoutList(k));
      return;
    }
    isLoadingVersions = true;
    void load(k, m, seq);
  });

  // `vanilla` has no builds; without an MC version nothing can be listed.
  function resolveWithoutList(k: LoaderKind) {
    if (!onchange) {
      if (loaderVersion !== null) loaderVersion = null;
      return;
    }
    // No MC + a modded loader is the parent's call to refuse (it owns the
    // "pick a Minecraft version first" message), so the request still goes.
    if (view?.phase === 'draft') requestCommit(k, null);
  }

  async function load(k: LoaderKind, m: string, seq: number): Promise<void> {
    error = null;
    let result: Awaited<ReturnType<typeof commands.listFabricLoaders>>;
    try {
      result =
        k === 'fabric'
          ? await commands.listFabricLoaders(m)
          : k === 'quilt'
            ? await commands.listQuiltLoaders(m)
            : k === 'neoforge'
              ? await commands.listNeoforgeLoaders(m)
              : await commands.listForgeLoaders(m);
    } catch (e) {
      if (seq !== loadSeq) return;
      isLoadingVersions = false;
      failList(e instanceof Error ? e.message : String(e));
      return;
    }
    // A newer (mc, loader) superseded this fetch — its list must not land on
    // top of the newer one's.
    if (seq !== loadSeq) return;
    isLoadingVersions = false;
    if (result.status !== 'ok') {
      failList(formatLoaderError(result.error));
      return;
    }
    if (result.data.length === 0) {
      failList(get(t)('instance.loader.errorUnavailable', { loader: displayLoader(k), mc: m }));
      return;
    }
    versions = result.data;
    // `resetToStable` is never needed here: a click already cleared the
    // version, and a cleared version resolves to the ecosystem's stable build
    // (e.g. Fabric 0.16.0 and Quilt 0.16.0 are unrelated, so a carried-over
    // number must not survive a switch — and with a cleared one it cannot).
    const stored = view ? view.version : loaderVersion;
    const next = resolveLoaderVersion(stored, result.data, false);
    if (!onchange) {
      if (next !== loaderVersion) loaderVersion = next;
      return;
    }
    // A click always ends in exactly one request. Without a view only a
    // genuinely stale committed version is corrected — and committed: a valid
    // version on screen with an invalid one saved was the Forge-install 404.
    if (view ? view.phase === 'draft' : next !== stored) requestCommit(k, next);
  }

  function pickLoader(k: LoaderKind) {
    if (!onchange) {
      if (k === loader) return;
      loader = k;
      // The old ecosystem's version means nothing to the new one. Clearing it
      // NOW keeps the bound draft from ever holding a cross-ecosystem pair (the
      // create form blocks Create on a missing version) and makes the ordinary
      // resolve land on the new ecosystem's stable build.
      loaderVersion = null;
      return;
    }
    if (k === shownLoader) return;
    pickSeq += 1;
    if (k !== loader) {
      view = { kind: k, version: null, phase: 'draft' };
      return;
    }
    // Back to the committed loader. If nothing is in flight and nothing was
    // written since the props last changed, abandoning the draft IS the
    // committed state. Otherwise a write may land (or has landed), so the
    // click must win with a real request — for the committed pair, not for
    // "recommended".
    if (outstanding === 0 && !writtenSinceProps) {
      view = null;
      return;
    }
    view = { kind: k, version: loaderVersion, phase: 'draft' };
  }

  function pickVersion(v: string) {
    if (!onchange) {
      loaderVersion = v;
      return;
    }
    if (v === shownVersion) return;
    pickSeq += 1;
    requestCommit(shownLoader, v);
  }

  // Mirror the previous <option> markup: stable entries carry the
  // "(recommended)" suffix, non-stable show the bare version. Recomputed
  // whenever the fetched `versions` list changes.
  const versionOptions = $derived(
    versions.map((lv) => ({
      value: lv.version,
      label: lv.stable ? $t('instance.loader.recommended', { version: lv.version }) : lv.version,
    })),
  );
</script>

<p id={groupLabelId} class="block text-xs uppercase text-secondary mb-1">
  {$t('instance.loader.label')}
</p>
<div class="flex gap-1 mb-3" role="group" aria-labelledby={groupLabelId}>
  {#each LOADER_KINDS as lk}
    <button
      type="button"
      class="flex-1 btn-sm"
      class:btn-primary={shownLoader === lk}
      class:btn-secondary={shownLoader !== lk}
      aria-pressed={shownLoader === lk}
      {disabled}
      onclick={() => pickLoader(lk)}
    >
      {displayLoader(lk)}
    </button>
  {/each}
</div>

{#if shownLoader !== 'vanilla' && (versions.length > 0 || isLoadingVersions)}
  <label class="block text-xs uppercase text-secondary mb-1" for={versionSelectId}>
    {$t('instance.loader.versionLabel')}
  </label>
  {#if isLoadingVersions}
    <div class="w-full mb-3 flex items-center gap-2 text-secondary">
      <Spinner
        size="sm"
        labelPlacement="right"
        label={$t('instance.loader.loadingVersions')}
        delayMs={150}
      />
    </div>
  {:else}
    <Select
      id={versionSelectId}
      class="w-full mb-3"
      value={shownVersion ?? ''}
      options={versionOptions}
      {disabled}
      onChange={(v) => pickVersion(String(v))}
    />
  {/if}
{/if}

<StatusMessage tone="danger" message={error} class="mb-2" />
