<script lang="ts">
  import type { ServerCardRow } from './server-card-adapter';
  import type { ModUpdateState } from '$lib/ipc/bindings';
  import { t } from '$lib/i18n';
  import ModCard from '$lib/mods/ModCard.svelte';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';

  // `reason` (mods quarantine; plugins pass null) renders as a sibling badge —
  // never inside ModCard. `updateState` stays null for plugins (a neighboring
  // session owns plugin update-check).
  let {
    card,
    reason = null,
    canToggle = true,
    updateState = null,
    checking = false,
    onOpenDetail = () => {},
    onToggle,
    onUninstall,
    onUpdate = () => {},
  }: {
    card: ServerCardRow;
    reason?: string | null;
    canToggle?: boolean;
    updateState?: ModUpdateState | null;
    checking?: boolean;
    onOpenDetail?: () => void;
    onToggle: () => void;
    onUninstall: () => void;
    onUpdate?: () => void;
  } = $props();

  const reasonLabel = $derived(
    reason === 'client_only'
      ? $t('servers.mods.setAsideClientOnly')
      : reason
        ? $t('servers.mods.setAside')
        : null,
  );
</script>

<div>
  <ModCard
    layout="list"
    summary={card.summary}
    installed={card.installed}
    onInstall={() => {}}
    {onOpenDetail}
    {onToggle}
    {onUninstall}
    {canToggle}
    {updateState}
    {onUpdate}
    {checking}
  />
  {#if reasonLabel}
    <div class="flex items-center gap-2 px-3 pb-0.5 text-xs">
      <StatusBadge variant="muted">{reasonLabel}</StatusBadge>
    </div>
  {/if}
</div>
