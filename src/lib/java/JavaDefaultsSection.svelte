<script lang="ts">
  // Settings → Game → Java (spec 2026-10-11-custom-java §6.2): one row per Java
  // major — the majors the installed instances need, and any row already saved
  // — each naming the Java that every instance and server needing that major
  // runs on unless it chose its own. «Lucerna's Java» removes the row. Its own
  // component, so the Game page's other tests keep their fixed IPC mocks.
  import { t } from '$lib/i18n';
  import { commands, type JavaChoice } from '$lib/ipc/bindings';
  import { generalDisplayed, writeGeneralVia } from '$lib/settings/app-settings.svelte';
  import SettingsField from '$lib/settings/SettingsField.svelte';
  import JavaChoiceField, { type IpcResult, type JavaSavedValue } from './JavaChoiceField.svelte';

  let needed = $state<number[] | null>(null);
  let neededFailed = $state(false);

  $effect(() => {
    void loadNeeded();
  });

  async function loadNeeded() {
    try {
      const r = await commands.javaRequiredMajors();
      if (r.status === 'ok') {
        needed = r.data;
        return;
      }
    } catch {
      // Falls through: the rows saved so far still show.
    }
    needed = [];
    neededFailed = true;
  }

  // Null while Settings is pending or unreadable: no row is claimed then — a
  // row shown as «Lucerna's Java» could be a Java Settings does name.
  const general = $derived(generalDisplayed());

  const rows = $derived.by(() => {
    const saved = general?.java_defaults ?? [];
    const majors = new Set<number>([...(needed ?? []), ...saved.map((d) => d.major)]);
    return [...majors]
      .sort((a, b) => b - a)
      .map((major) => ({ major, row: saved.find((d) => d.major === major) ?? null }));
  });

  function saveFor(major: number) {
    return async (choice: JavaChoice | null): Promise<IpcResult<JavaSavedValue>> => {
      const path = choice?.kind === 'custom' ? choice.path : null;
      const r = await writeGeneralVia(
        () => commands.setJavaDefault(major, path),
        (d) => d.settings,
      );
      if (r.status !== 'ok') return r;
      const row = r.data.settings.java_defaults?.find((d) => d.major === major) ?? null;
      const value: JavaChoice | null = row ? { kind: 'custom', path: row.path } : null;
      return { status: 'ok', data: { value, report: r.data.report } };
    };
  }
</script>

<SettingsField anchor="game.java">
  <div class="flex flex-col gap-2" data-testid="java-defaults">
    <h3 class="font-medium text-sm text-primary">{$t('settings.general.java.title')}</h3>
    <p class="text-xs text-muted">{$t('settings.general.java.intro')}</p>
    {#if general === null}
      <!-- Pending or unreadable: the panel above says which, with Retry. -->
    {:else if needed === null}
      <span class="text-xs text-placeholder">{$t('common.loading')}</span>
    {:else}
      {#if neededFailed}
        <!-- The saved rows still show; the ones the instances need may be missing. -->
        <p class="text-xs text-secondary" data-testid="java-defaults-failed">
          {$t('settings.general.java.majorsFailed')}
        </p>
      {:else if rows.length === 0}
        <p class="text-xs text-secondary" data-testid="java-defaults-empty">
          {$t('settings.general.java.empty')}
        </p>
      {/if}
      {#each rows as r (r.major)}
        <JavaChoiceField
          reloadKey={String(r.major)}
          value={r.row ? { kind: 'custom', path: r.row.path } : null}
          label={$t('settings.general.java.rowLabel', { major: r.major })}
          loadRequired={async () => ({ status: 'ok', data: r.major })}
          save={saveFor(r.major)}
          lucernaWrites={null}
          testid="java-default-{r.major}"
        />
      {/each}
    {/if}
  </div>
</SettingsField>
