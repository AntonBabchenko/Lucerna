<script lang="ts">
  import { t } from '$lib/i18n';
  import StatusBadge from '$lib/ui/cards/StatusBadge.svelte';
</script>

<!--
  A WORD on a pill is a status, not a count — so this is a StatusBadge, not a
  CountPill (§9: "`StatusBadge` is the single status pill"). `info` is the
  accent-soft variant, which is what the hand-rolled `bg-accent` + white label
  was reaching for. Exactly the migration KeyEditRow's STATE_VARIANT comment
  records: "the hand-rolled tones this replaced had drifted off the design
  system (raw `bg-success/10` where the token is `bg-success-bg`,
  `rounded-full` and `text-[11px]` where every other status pill in the app is
  `rounded` / `text-xs`)."
-->
<span class="shrink-0">
  <StatusBadge variant="info" testid="manage-active-badge"
    >{$t('instance.manage.activeBadge')}</StatusBadge
  >
</span>
