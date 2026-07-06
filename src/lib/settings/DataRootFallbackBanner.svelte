<script lang="ts">
  // Prominent, persistent, NON-dismissible banner shown app-wide whenever the
  // configured data root is unavailable and the app fell back to a temporary
  // default location (§7 of the data-root design doc). Deliberately has no ×:
  // per DESIGN.md §10's gate-banner carve-out, a banner whose condition
  // actively blocks other actions (create/Play — see data-root-gating.ts)
  // must stay visible for as long as the condition holds, so there is nothing
  // useful to "dismiss" — the user must reconnect the folder and restart.
  import { Icon } from '$lib/ui/icons';
  import { t } from '$lib/i18n';

  let { configuredPath }: { configuredPath: string } = $props();
</script>

<div
  class="bg-warning-bg border-b border-warning-text text-warning-text px-4 py-2 flex items-center gap-2"
  data-testid="data-root-fallback-banner"
  role="alert"
>
  <Icon name="warning" class="shrink-0" />
  <span class="text-sm">{$t('page.dataRootFallback.banner', { path: configuredPath })}</span>
</div>
