<script lang="ts">
  // Renders one ShaderBannerState (shader-loader-hint.ts) inside the shared
  // accent Banner. Presentational: no IPC; the only logic is choosing strings
  // and controls, plus the pending delay. Actions are small buttons like every
  // Banner consumer's; OptiFine opens its site, so it carries externalLink.
  import { t } from '$lib/i18n';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { displayLoader } from '$lib/instances/loader-display';
  import Banner from '$lib/ui/Banner.svelte';
  import BusyButton from '$lib/ui/BusyButton.svelte';
  import { Icon } from '$lib/ui/icons';
  import Spinner from '$lib/ui/Spinner.svelte';
  import type {
    InstallTarget,
    Loader,
    ModrinthShaderLoader,
    OptifineForgeBuild,
    ShaderBannerState,
    ShaderLoaderId,
  } from './shader-loader-hint';

  // A fast answer must not flash a "checking" banner first.
  const PENDING_DELAY_MS = 300;

  let {
    hint,
    loader,
    mcVersion,
    rechecking,
    onOpenIris,
    onOpenOculus,
    onOpenOptifine,
    onRecheck,
    onManageLoader,
  }: {
    hint: ShaderBannerState;
    loader: Loader;
    mcVersion: string | null;
    rechecking: boolean;
    onOpenIris: () => void;
    onOpenOculus: () => void;
    onOpenOptifine: () => void;
    onRecheck: () => void;
    onManageLoader: () => void;
  } = $props();

  let pendingShown = $state(false);
  $effect(() => {
    if (hint.kind !== 'pending') {
      pendingShown = false;
      return;
    }
    const timer = setTimeout(() => {
      pendingShown = true;
    }, PENDING_DELAY_MS);
    return () => clearTimeout(timer);
  });

  const visible = $derived(hint.kind !== 'hidden' && (hint.kind !== 'pending' || pendingShown));

  const NAME_KEY: Record<ShaderLoaderId, TranslationKey> = {
    iris: 'addons.shaderLoaderHint.iris',
    oculus: 'addons.shaderLoaderHint.oculus',
    optifine: 'addons.shaderLoaderHint.optifine',
  };
  const NONE_FOUND_KEY: Record<'iris' | 'iris_oculus' | 'forge', TranslationKey> = {
    iris: 'addons.shaderLoaderHint.noneFoundIris',
    iris_oculus: 'addons.shaderLoaderHint.noneFoundIrisOculus',
    forge: 'addons.shaderLoaderHint.noneFoundForge',
  };

  const profile = $derived(loader && mcVersion ? `${displayLoader(loader)} ${mcVersion}` : '');

  function shaderLabel(id: ShaderLoaderId, optifine: OptifineForgeBuild | null): string {
    if (id === 'optifine' && optifine === 'preview') {
      return $t('addons.shaderLoaderHint.optifinePreviewLabel');
    }
    return $t(NAME_KEY[id]);
  }

  function shaderNames(ids: ModrinthShaderLoader[]): string {
    return ids.map((id) => $t(NAME_KEY[id])).join(', ');
  }

  function loaderNames(targets: InstallTarget[]): string {
    return targets.map((target) => displayLoader(target)).join(', ');
  }

  function optifineNote(build: OptifineForgeBuild): string {
    return build === 'preview'
      ? $t('addons.shaderLoaderHint.optifineForgePreview', { mc: mcVersion ?? '' })
      : $t('addons.shaderLoaderHint.optifineForgeRelease', { mc: mcVersion ?? '' });
  }
</script>

{#snippet uncheckedLine(names: string)}
  <div class="flex flex-wrap items-center gap-2">
    <p class="text-xs text-muted">{$t('addons.shaderLoaderHint.unchecked', { names })}</p>
    <BusyButton class="btn-ghost btn-sm" busy={rechecking} onclick={onRecheck}>
      {$t('addons.shaderLoaderHint.recheck')}
    </BusyButton>
  </div>
{/snippet}

{#snippet chooseModLoader()}
  <button type="button" class="btn-secondary btn-sm" onclick={onManageLoader}>
    {$t('addons.shaderLoaderHint.chooseModLoader')}
  </button>
{/snippet}

{#if visible}
  <div class="px-3 pt-3" data-testid="shader-loader-hint">
    <Banner tone="accent" role="note" class="text-sm text-secondary">
      <div class="space-y-2">
        {#if hint.kind === 'pending'}
          <p>{$t('addons.shaderLoaderHint.needLoader')}</p>
          <Spinner
            size="sm"
            delayMs={0}
            labelPlacement="right"
            label={$t('addons.shaderLoaderHint.checking')}
          />
        {:else if hint.kind === 'no_instance'}
          <p>
            {$t('addons.shaderLoaderHint.generic')}
            {$t('addons.shaderLoaderHint.noInstanceHint')}
          </p>
        {:else if hint.kind === 'generic'}
          <p>{$t('addons.shaderLoaderHint.generic')}</p>
        {:else if hint.kind === 'options'}
          <p>
            {hint.buttons.length === 1
              ? $t('addons.shaderLoaderHint.introOne')
              : $t('addons.shaderLoaderHint.intro')}
          </p>
          <div class="flex flex-wrap items-center gap-2">
            {#each hint.buttons as id, i (id)}
              {#if i > 0}
                <span class="text-muted">{$t('addons.shaderLoaderHint.or')}</span>
              {/if}
              {#if id === 'optifine'}
                <button
                  type="button"
                  class="btn-secondary btn-sm inline-flex items-center gap-1"
                  onclick={onOpenOptifine}
                >
                  {$t('addons.shaderLoaderHint.optifine')}<Icon name="externalLink" size={14} />
                </button>
              {:else}
                <button
                  type="button"
                  class="btn-secondary btn-sm"
                  onclick={id === 'iris' ? onOpenIris : onOpenOculus}
                >
                  {$t(NAME_KEY[id])}
                </button>
              {/if}
            {/each}
          </div>
          {#if hint.optifine}
            <p class="text-xs text-muted">{optifineNote(hint.optifine)}</p>
          {/if}
          {#if hint.unchecked.length > 0}
            {@render uncheckedLine(shaderNames(hint.unchecked))}
          {/if}
        {:else if hint.kind === 'unchecked'}
          <p>{$t('addons.shaderLoaderHint.needLoader')}</p>
          {@render uncheckedLine(shaderNames(hint.unchecked))}
        {:else if hint.kind === 'quilt_fabric_only'}
          <p>{$t('addons.shaderLoaderHint.quiltFabricOnly', { mc: mcVersion ?? '' })}</p>
          <p>{$t('addons.shaderLoaderHint.noneFoundTail')}</p>
        {:else if hint.kind === 'none_found'}
          <p>{$t(NONE_FOUND_KEY[hint.variant], { profile })}</p>
          <p>{$t('addons.shaderLoaderHint.noneFoundTail')}</p>
        {:else if hint.kind === 'vanilla_options'}
          <p>{$t('addons.shaderLoaderHint.vanillaIntro', { mc: mcVersion ?? '' })}</p>
          <ul class="list-disc pl-5">
            {#each hint.lines as line (line.loader)}
              <li>
                {$t('addons.shaderLoaderHint.vanillaOption', {
                  modLoader: displayLoader(line.loader),
                  shaderLoaders: line.shaders
                    .map((id) => shaderLabel(id, line.optifine))
                    .join(` ${$t('addons.shaderLoaderHint.or')} `),
                })}
              </li>
            {/each}
          </ul>
          <div>{@render chooseModLoader()}</div>
          {#if hint.unchecked.length > 0}
            {@render uncheckedLine(loaderNames(hint.unchecked))}
          {/if}
        {:else if hint.kind === 'vanilla_unchecked'}
          <p>{$t('addons.shaderLoaderHint.vanillaUnchecked', { mc: mcVersion ?? '' })}</p>
          <div class="flex flex-wrap items-center gap-2">
            <BusyButton class="btn-ghost btn-sm" busy={rechecking} onclick={onRecheck}>
              {$t('addons.shaderLoaderHint.recheck')}
            </BusyButton>
            {@render chooseModLoader()}
          </div>
        {:else if hint.kind === 'vanilla_none_found'}
          <p>{$t('addons.shaderLoaderHint.vanillaNoneFound', { mc: mcVersion ?? '' })}</p>
          <p>{$t('addons.shaderLoaderHint.noneFoundTail')}</p>
        {/if}
      </div>
    </Banner>
  </div>
{/if}
