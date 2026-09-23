<script lang="ts">
  // "What's new" — renders the embedded, parsed CHANGELOG.md in the interface
  // language. Pure presentation over the English structure: the active
  // locale's translation (./locales/<code>.md) is laid over it bullet by
  // bullet, and wherever it is absent the English text is shown *and said to
  // be English* — per version, or once for the whole panel when the locale
  // has no changelog at all or it could not be loaded.
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
  let { entries, headingLevel = 4 }: { entries: Changelog; headingLevel?: 3 | 4 } = $props();
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

  function openUrl(url: string): void {
    // The changelog is our own build-time artifact, but a parsed link is still
    // data: the chokepoint refuses anything but https:// and says so.
    void openExternalHttps(url);
  }
</script>

<section class="space-y-6 text-sm selectable">
  {#if visible.length === 0}
    <p class="text-muted">{$t('settings.changelog.empty')}</p>
  {:else}
    {#if panelNote}
      <p class="text-xs text-muted" data-testid="changelog-fallback-note">{$t(panelNote)}</p>
    {/if}
    {#each visible as ver (ver.version)}
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
      </article>
    {/each}
  {/if}
</section>
