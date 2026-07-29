<script lang="ts">
  import { t } from '$lib/i18n';
  import { Icon } from '$lib/ui/icons';
  import { openEula } from '$lib/servers/eula-link';

  // `label` lets the diagnosis banner use a standalone phrase ("Read the
  // Minecraft EULA") where there is no sentence to embed the link into.
  let { label }: { label?: string } = $props();
</script>

<!-- MUST NOT be nested inside the <label> of the acceptance checkbox: the
     label's activation behaviour would tick "I accept" when the user only
     clicked through to read the agreement, and `stopPropagation` does not
     prevent it. Callers pair the checkbox with a `for=`/`id=` label instead. -->
<button
  type="button"
  class="btn-link inline-flex items-center gap-1"
  data-testid="eula-link"
  aria-label={$t('servers.wizard.eulaLinkAria')}
  onclick={openEula}
>
  {label ?? $t('servers.wizard.eulaLink')}
  <Icon name="externalLink" size={12} />
</button>
