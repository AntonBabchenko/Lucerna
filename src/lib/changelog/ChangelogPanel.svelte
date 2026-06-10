<script lang="ts">
  // "What's new" — renders the embedded, parsed CHANGELOG.md. Pure
  // presentation: it receives the already-parsed model and only handles
  // layout, heading localization, and opening a version's GitHub link.
  import type { TranslationKey } from '$lib/i18n/keys.generated';
  import { t } from '$lib/i18n';
  import type { Changelog, SectionKind } from './types';

  let { entries }: { entries: Changelog } = $props();

  // Localized labels for the known Keep-a-Changelog kinds. 'other' is absent
  // on purpose — those sections render their verbatim heading.
  const SECTION_KEY: Record<Exclude<SectionKind, 'other'>, TranslationKey> = {
    added: 'settings.changelog.sections.added',
    changed: 'settings.changelog.sections.changed',
    fixed: 'settings.changelog.sections.fixed',
    deprecated: 'settings.changelog.sections.deprecated',
    removed: 'settings.changelog.sections.removed',
    security: 'settings.changelog.sections.security',
  };

  // Versions with no content (e.g. an empty [Unreleased]) are dropped, and
  // each section's heading is pre-localized here so the markup stays simple
  // and reactive to locale changes (via the `$t` store dependency).
  const visible = $derived(
    entries
      .filter((v) => v.sections.length > 0)
      .map((v) => ({
        ...v,
        sections: v.sections.map((s) => ({
          ...s,
          label: s.kind === 'other' ? s.heading : $t(SECTION_KEY[s.kind]),
        })),
      })),
  );

  function openUrl(url: string): void {
    void import('@tauri-apps/plugin-opener').then((m) => m.openUrl(url));
  }
</script>

<section class="space-y-6 text-sm selectable">
  <h3 class="text-base font-semibold text-primary">{$t('settings.tabs.changelog')}</h3>

  {#if visible.length === 0}
    <p class="text-muted">{$t('settings.changelog.empty')}</p>
  {:else}
    {#each visible as ver (ver.version)}
      <article class="space-y-2">
        <header class="flex items-baseline justify-between gap-2">
          {#if ver.url}
            {@const href = ver.url}
            <button
              type="button"
              class="btn-tertiary font-medium text-primary"
              title={href}
              onclick={() => openUrl(href)}
            >
              v{ver.version}
            </button>
          {:else}
            <span class="font-medium text-primary">v{ver.version}</span>
          {/if}
          {#if ver.date}<span class="text-xs text-muted">{ver.date}</span>{/if}
        </header>

        {#each ver.sections as sec (sec.heading)}
          <div class="space-y-1">
            <h4 class="text-xs font-semibold uppercase tracking-wide text-secondary">
              {sec.label}
            </h4>
            <ul class="list-disc space-y-1 pl-5 text-secondary">
              {#each sec.items as item, i (i)}
                <li>{item}</li>
              {/each}
            </ul>
          </div>
        {/each}
      </article>
    {/each}
  {/if}
</section>
