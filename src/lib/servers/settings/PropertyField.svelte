<script lang="ts">
  import { t } from '$lib/i18n';
  import Select, { type SelectOption } from '$lib/ui/Select.svelte';
  import { Icon } from '$lib/ui/icons';
  import { tooltip } from '$lib/ui/tooltip';
  import { keyToI18n, type PropSpec } from './properties-catalog';

  let {
    spec,
    value,
    onChange,
  }: {
    spec: PropSpec;
    value: string;
    onChange: (value: string) => void;
  } = $props();

  const i18n = $derived(keyToI18n(spec.key));
  const label = $derived($t(`servers.props.${i18n}.label` as never));
  const desc = $derived($t(`servers.props.${i18n}.desc` as never));
  const changed = $derived(value !== spec.default);
  const fieldId = $derived(`sp-${spec.key.replace(/\./g, '-')}`);

  const enumOptions = $derived<SelectOption[]>(
    spec.type.kind === 'enum'
      ? spec.type.values.map((v) => ({
          value: v,
          label: $t(`servers.propEnums.${i18n}.${v}` as never),
        }))
      : [],
  );
</script>

<div class="grid grid-cols-[minmax(0,1fr)_minmax(0,320px)] items-start gap-x-4 gap-y-1 py-2">
  <div class="flex flex-col gap-0.5 min-w-0">
    <div class="flex items-center gap-1.5">
      <label class="text-sm font-medium text-primary" for={fieldId}>{label}</label>
      <button
        type="button"
        class="shrink-0 cursor-help text-muted hover:text-secondary"
        aria-label={desc}
        use:tooltip={{ text: desc, describe: false }}
      >
        <Icon name="info" size={13} />
      </button>
      <code class="text-[11px] text-muted truncate">{spec.key}</code>
      {#if changed}
        <span class="text-[10px] text-accent">• {$t('servers.propsUi.changed')}</span>
      {/if}
    </div>
    <p class="text-[11px] text-muted">
      {$t('servers.propsUi.default', {
        value: spec.type.kind === 'string' && spec.default === '' ? '∅' : spec.default,
      })}
    </p>
  </div>

  <div class="flex items-center gap-2 justify-self-end w-full max-w-[320px]">
    {#if spec.type.kind === 'bool'}
      <input
        id={fieldId}
        type="checkbox"
        class="accent-accent cursor-pointer"
        checked={value === 'true'}
        onchange={(e) => onChange(e.currentTarget.checked ? 'true' : 'false')}
      />
    {:else if spec.type.kind === 'enum'}
      <Select
        id={fieldId}
        value={value === '' ? spec.default : value}
        options={enumOptions}
        onChange={(v) => onChange(String(v))}
        ariaLabel={label}
        class="w-full"
      />
    {:else if spec.type.kind === 'int'}
      <input
        id={fieldId}
        type="number"
        min={spec.type.min}
        max={spec.type.max}
        class="h-8 w-full rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        {value}
        oninput={(e) => onChange(e.currentTarget.value)}
      />
    {:else}
      <input
        id={fieldId}
        type={spec.type.secret ? 'password' : 'text'}
        autocomplete="off"
        class="h-8 w-full rounded border border-border-emphasis bg-surface px-3 text-sm text-primary"
        {value}
        oninput={(e) => onChange(e.currentTarget.value)}
      />
    {/if}

    {#if changed}
      <button
        type="button"
        class="btn-ghost btn-xs shrink-0"
        aria-label={$t('servers.propsUi.reset')}
        use:tooltip={{ text: $t('servers.propsUi.reset'), describe: false }}
        onclick={() => onChange(spec.default)}
      >
        <Icon name="refresh" size={14} />
      </button>
    {/if}
  </div>
</div>
