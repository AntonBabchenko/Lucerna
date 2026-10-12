<script lang="ts">
  // The Shaders tab's shader-loader hint: reads which shader loaders the
  // profile already has and asks (through shader-loader-availability) which
  // ones have a version for its mod loader + Minecraft, then hands one state to
  // ShaderLoaderBanner. Mounted by AddonsTab only while the Shaders segment is
  // showing, so every read and listener here lives exactly as long as the hint.
  import { onDestroy } from 'svelte';
  import { commands, events, type InstalledMod } from '$lib/ipc/bindings';
  import { listenUntilDestroyed } from '$lib/ipc/listen';
  import { debounceTrailing } from '$lib/ui/debounce';
  import ShaderLoaderBanner from './ShaderLoaderBanner.svelte';
  import { checkPlan, type Loader, type Outcomes, shaderBannerState } from './shader-loader-hint';
  import { cachedOutcomes, runChecks } from './shader-loader-availability';

  let {
    instanceId,
    mcVersion,
    loader,
    onOpenIris,
    onOpenOculus,
    onOpenOptifine,
    onManageLoader,
  }: {
    instanceId: string | null;
    mcVersion: string | null;
    loader: Loader;
    onOpenIris: () => void;
    onOpenOculus: () => void;
    onOpenOptifine: () => void;
    onManageLoader: () => void;
  } = $props();

  // The installed list, tagged with the profile it was read for: a list read
  // for another profile counts as not read yet, so a switch can neither hide
  // nor show the hint on the previous profile's mods.
  let installed = $state<{ instanceId: string; mods: InstalledMod[] } | null>(null);

  async function readInstalled(id: string) {
    let mods: InstalledMod[] = [];
    try {
      const r = await commands.modsListInstalled(id);
      if (r.status === 'ok') mods = r.data;
    } catch {
      // A failed read counts as "no shader loader detected" (below): the hint
      // may then show next to an installed loader. The other way round it
      // would hide for good on a list we never got.
    }
    if (instanceId === id) installed = { instanceId: id, mods };
  }

  $effect(() => {
    if (instanceId) void readInstalled(instanceId);
  });

  // Keep the hint live: it disappears the moment a shader loader is installed.
  // Burst coalescing: a with-deps install emits one event per jar.
  const debouncedRead = debounceTrailing(() => {
    if (instanceId) void readInstalled(instanceId);
  }, 150);
  onDestroy(debouncedRead.cancel);
  listenUntilDestroyed([
    events.modInstalled.listen(debouncedRead.call),
    events.modUninstalled.listen(debouncedRead.call),
    events.modToggle.listen(debouncedRead.call),
  ]);

  const installedForProfile = $derived(
    installed !== null && installed.instanceId === instanceId ? installed.mods : null,
  );

  // Answers, tagged with the profile key they were asked for. A key that is no
  // longer current counts as pending, so new props never meet old answers.
  const profileKey = $derived(
    instanceId && loader && mcVersion ? `${instanceId}\n${loader}\n${mcVersion}` : null,
  );
  let checked = $state<{ key: string; outcomes: Outcomes } | null>(null);
  let recheckNonce = $state(0);
  let rechecking = $state(false);

  $effect(() => {
    const key = profileKey;
    // biome-ignore lint/correctness/noUnusedVariables: reactive read — "Check again" re-runs this
    const _nonce = recheckNonce;
    if (key === null || loader === null || !mcVersion) return;
    const plan = checkPlan(loader);
    const cached = cachedOutcomes(plan, mcVersion);
    if (cached !== null) {
      checked = { key, outcomes: cached };
      rechecking = false;
      return;
    }
    void runChecks(plan, mcVersion).then((outcomes) => {
      if (profileKey !== key) return;
      checked = { key, outcomes };
      rechecking = false;
    });
  });

  const outcomes = $derived(
    checked !== null && checked.key === profileKey ? checked.outcomes : null,
  );

  const hint = $derived(
    shaderBannerState({
      instanceSelected: instanceId !== null,
      loader,
      mcVersion,
      installed: installedForProfile,
      outcomes,
    }),
  );

  // The previous answers stay on screen while a re-check runs; only the
  // button shows it is busy. Successes are remembered, so only the checks that
  // could not be answered go back out.
  function recheck() {
    rechecking = true;
    recheckNonce += 1;
  }
</script>

<ShaderLoaderBanner
  {hint}
  {loader}
  {mcVersion}
  {rechecking}
  {onOpenIris}
  {onOpenOculus}
  {onOpenOptifine}
  onRecheck={recheck}
  {onManageLoader}
/>
