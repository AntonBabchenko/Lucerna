<script lang="ts">
  // "What's new" — renders the embedded, parsed CHANGELOG.md in the interface
  // language. Pure presentation over the English structure: the active
  // locale's translation (./locales/<code>.md) is laid over it bullet by
  // bullet, and wherever it is absent the English text is shown *and said to
  // be English* — per version, or once for the whole panel when the locale
  // has no changelog at all or it could not be loaded.
  import { SvelteSet } from 'svelte/reactivity';
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { locale, t } from '$lib/i18n';
  import { tooltip } from '$lib/ui/tooltip';
  import { Icon } from '$lib/ui/icons';
  import { openExternalHttps } from '$lib/ui/safe-open';
  import { parseInline } from './inline';
  import { CHANGELOG_SOURCE_LOCALE } from './locales';
  import { asSourceLanguage, localizeChangelog } from './localize';
  import { changelogTranslations, ensureChangelogTranslation } from './translation.svelte';
  import type { Changelog, SectionKind } from './types';

  // Heading levels: versions are one level under the host's heading, sections
  // one under that — Settings → Updates (an h3 block) gets h4 / h5, the
  // What's-new dialog (an h2 title) gets h3 / h4. No level is skipped either way.
  // `installedVersion` = the installed version (Settings passes it, the What's-new dialog does
  // not): every version then opens and closes on its own, and this one starts open.
  let {
    entries,
    headingLevel = 4,
    installedVersion,
  }: { entries: Changelog; headingLevel?: 3 | 4; installedVersion?: string } = $props();
  const uid = $props.id();
  const versionTag = $derived(`h${headingLevel}`);
  const sectionTag = $derived(`h${headingLevel + 1}`);

  // Localized labels for the known Keep-a-Changelog kinds. 'other' is absent
  // on purpose — those sections render their (possibly translated) heading.
  const SECTION_KEY: Record<Exclude<SectionKind, 'other'>, TranslationKey> = {
    added: 'settings.changelog.sections.added',
    changed: 'settings.changelog.sections.changed',
    fixed: 'settings.changelog.sections.fixed',
    deprecated: 'settings.changelog.sections.deprecated',
    removed: 'settings.changelog.sections.removed',
    security: 'settings.changelog.sections.security',
  };

  const active = $derived($locale ?? CHANGELOG_SOURCE_LOCALE);
  const isSource = $derived(active === CHANGELOG_SOURCE_LOCALE);
  $effect(() => {
    void ensureChangelogTranslation(active);
  });
  const load = $derived(isSource ? null : changelogTranslations[active]);

  // English structure, the locale's text. Until the translation has loaded
  // (`load` undefined) the English text shows with no note: nothing is known
  // yet, so nothing is claimed.
  const display = $derived(
    isSource || !load
      ? asSourceLanguage(entries)
      : localizeChangelog(entries, load.status === 'ready' ? load.entries : null),
  );

  // When the locale has no changelog, or it failed to load, every version is
  // English for one reason — say it once, not nineteen times.
  const panelNote: TranslationKey | null = $derived(
    !load || load.status === 'ready'
      ? null
      : load.status === 'missing'
        ? 'settings.changelog.fallback.noTranslation'
        : 'settings.changelog.fallback.loadFailed',
  );

  // Versions with no content (e.g. an empty [Unreleased]) are dropped, and
  // each section's heading is pre-localized here so the markup stays simple
  // and reactive to locale changes (via the `$t` store dependency).
  const visible = $derived(
    display
      .filter((v) => v.sections.length > 0)
      .map((v) => ({
        ...v,
        note: (panelNote
          ? null
          : v.coverage === 'none'
            ? 'settings.changelog.fallback.versionUntranslated'
            : v.coverage === 'partial'
              ? 'settings.changelog.fallback.versionPartial'
              : null) as TranslationKey | null,
        sections: v.sections.map((s) => ({
          ...s,
          label: s.kind === 'other' ? s.heading : $t(SECTION_KEY[s.kind]),
        })),
      })),
  );

  // UPD-09: which version starts open. The installed one when it is listed;
  // otherwise the newest released one (a dev build's version may not be in the
  // file yet). Every other version — older ones, and an [Unreleased] entry —
  // starts closed. Without installedVersion nothing folds (null).
  const openVersion = $derived.by((): string | null => {
    if (!installedVersion) return null;
    if (visible.some((v) => v.version === installedVersion)) return installedVersion;
    const released = visible.find((v) => v.version.toLowerCase() !== 'unreleased');
    return released?.version ?? visible[0]?.version ?? null;
  });

  // The versions the user opened or closed, against the default (only
  // openVersion open). Kept as a difference, not a copy of the open set, so the
  // default can still follow openVersion.
  const toggled = new SvelteSet<string>();
  const isOpen = (version: string): boolean => (version === openVersion) !== toggled.has(version);
  function toggle(version: string): void {
    if (toggled.has(version)) toggled.delete(version);
    else toggled.add(version);
  }

  function openUrl(url: string): void {
    // The changelog is our own build-time artifact, but a parsed link is still
    // data: the chokepoint refuses anything but https:// and says so.
    void openExternalHttps(url);
  }
</script>

{#snippet versionBlock(ver: (typeof visible)[number])}
  <article class="space-y-2">
    <header class="flex items-baseline justify-between gap-2">
      <svelte:element this={versionTag} class="font-medium">
        {#if ver.url}
          {@const href = ver.url}
          <button
            type="button"
            class="btn-link inline-flex items-center gap-1"
            use:tooltip={href}
            onclick={() => openUrl(href)}
          >
            v{ver.version}
            <Icon name="externalLink" size={12} />
          </button>
        {:else}
          <span class="text-primary">v{ver.version}</span>
        {/if}
      </svelte:element>
      {#if ver.date}<span class="text-xs text-muted">{ver.date}</span>{/if}
    </header>
    {@render versionBody(ver)}
  </article>
{/snippet}

<!-- Settings: the version is a disclosure of its own. The manual toggle
     (caret / chevronDown), not <details>: inside a <summary> the heading is
     exposed differently per engine, and the version must stay an h4
     (DESIGN §13). The body renders only while open and is named in
     aria-controls only then. -->
{#snippet foldedVersion(ver: (typeof visible)[number], i: number)}
  {@const open = isOpen(ver.version)}
  {@const bodyId = `${uid}-v${i}`}
  <article class="space-y-2">
    <header class="flex items-baseline justify-between gap-2">
      <svelte:element this={versionTag} class="font-medium">
        <button
          type="button"
          class="inline-flex items-center gap-1 rounded text-primary hover:underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2"
          aria-expanded={open}
          aria-controls={open ? bodyId : undefined}
          onclick={() => toggle(ver.version)}
        >
          <Icon name={open ? 'chevronDown' : 'caret'} size={14} />
          <span>v{ver.version}</span>
        </button>
      </svelte:element>
      {#if ver.date}<span class="text-xs text-muted">{ver.date}</span>{/if}
    </header>
    {#if open}
      <div id={bodyId} class="space-y-2 pl-5">
        {@render versionBody(ver)}
        {#if ver.url}
          {@const href = ver.url}
          <p>
            <button
              type="button"
              class="btn-link inline-flex items-center gap-1 text-xs"
              use:tooltip={href}
              onclick={() => openUrl(href)}
            >
              {$t('settings.changelog.compareLink')}
              <Icon name="externalLink" size={12} />
            </button>
          </p>
        {/if}
      </div>
    {/if}
  </article>
{/snippet}

{#snippet versionBody(ver: (typeof visible)[number])}
  {#if ver.note}
    <p class="text-xs text-muted" data-testid="changelog-version-note">{$t(ver.note)}</p>
  {/if}

  {#each ver.sections as sec, si (si)}
    <div class="space-y-1">
      <svelte:element
        this={sectionTag}
        class="text-xs font-semibold uppercase tracking-wide text-secondary"
      >
        {sec.label}
      </svelte:element>
      <ul class="list-disc space-y-1 pl-5 text-secondary">
        {#each sec.items as item, i (i)}
          <li>
            {#each parseInline(item.text) as seg, k (k)}
              {#if seg.bold && seg.code}
                <strong><code class="font-mono text-[0.9em]">{seg.value}</code></strong>
              {:else if seg.bold}
                <strong class="font-medium text-primary">{seg.value}</strong>
              {:else if seg.code}
                <code class="font-mono text-[0.9em]">{seg.value}</code>
              {:else}{seg.value}{/if}
            {/each}
          </li>
        {/each}
      </ul>
    </div>
  {/each}
{/snippet}

<section class="space-y-6 text-sm selectable">
  {#if visible.length === 0}
    <p class="text-muted">{$t('settings.changelog.empty')}</p>
  {:else}
    {#if panelNote}
      <p class="text-xs text-muted" data-testid="changelog-fallback-note">{$t(panelNote)}</p>
    {/if}
    {#if openVersion === null}
      {#each visible as ver (ver.version)}
        {@render versionBlock(ver)}
      {/each}
    {:else}
      <div class="space-y-3">
        {#each visible as ver, i (ver.version)}
          {@render foldedVersion(ver, i)}
        {/each}
      </div>
    {/if}
  {/if}
</section>
