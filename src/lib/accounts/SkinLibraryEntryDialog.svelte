<script lang="ts">
  // Save/edit dialog for a skin-library entry: name + model variant + (when the
  // surface knows the account's capes) a per-skin cape. Purely presentational —
  // the actual save/update IPC stays with the caller, which keeps this dialog
  // open on error exactly like AddOfflineAccountDialog.
  import Modal from '$lib/ui/Modal.svelte';
  import Select, { type SelectOption } from '$lib/ui/Select.svelte';
  import SegmentedControl from '$lib/ui/SegmentedControl.svelte';
  import { t } from '$lib/i18n';
  import type { CapeInfo, SkinVariant } from '$lib/ipc/bindings';

  let {
    title,
    initial,
    capes = null,
    busy = false,
    error = null,
    onCancel,
    onSubmit,
  }: {
    /** Already-localized dialog title (save vs edit). */
    title: string;
    initial: { name: string; variant: SkinVariant; capeId: string | null };
    /** Owned capes, or null to hide the cape row (offline / editor-standalone). */
    capes?: CapeInfo[] | null;
    busy?: boolean;
    /** Backend error from the last submit, or null. Rendered inside the modal. */
    error?: string | null;
    onCancel: () => void;
    onSubmit: (v: { name: string; variant: SkinVariant; capeId: string | null }) => void;
  } = $props();

  // svelte-ignore state_referenced_locally — the prop seeds the editable state;
  // the dialog is mounted fresh per open, so capturing the initial value is
  // exactly the intent (same pattern as SkinEditorModal).
  let name = $state(initial.name);
  // svelte-ignore state_referenced_locally
  let variant = $state<SkinVariant>(initial.variant);
  // svelte-ignore state_referenced_locally
  let capeId = $state<string | null>(initial.capeId);

  const trimmed = $derived(name.trim());
  const canSubmit = $derived(trimmed.length > 0 && !busy);

  // Sentinel for the "no cape" row — Select values must be primitives.
  const NONE = '__none__';
  const capeOptions = $derived.by<SelectOption[]>(() => {
    const owned = capes ?? [];
    const opts: SelectOption[] = [
      { value: NONE, label: $t('skinLibrary.capeNone') },
      ...owned.map((c) => ({ value: c.id, label: c.alias ?? c.id })),
    ];
    // A saved cape the current account doesn't own must stay representable,
    // otherwise opening the dialog would silently drop it.
    if (capeId !== null && !owned.some((c) => c.id === capeId)) {
      opts.push({ value: capeId, label: capeId });
    }
    return opts;
  });

  function submit() {
    if (!canSubmit) return;
    onSubmit({ name: trimmed, variant, capeId });
  }
</script>

<Modal
  ariaLabelledby="skin-lib-entry-title"
  onClose={onCancel}
  panelClass="w-[440px] p-5 flex flex-col gap-3"
>
  <h3 id="skin-lib-entry-title" class="font-semibold text-primary text-base">{title}</h3>

  <form
    class="flex flex-col gap-3"
    onsubmit={(e) => {
      e.preventDefault();
      submit();
    }}
  >
    <div class="flex flex-col gap-1">
      <label class="text-xs text-secondary" for="skin-lib-name">
        {$t('skinLibrary.nameLabel')}
      </label>
      <input
        id="skin-lib-name"
        data-autofocus
        class="border rounded px-2 py-1 text-sm w-full"
        bind:value={name}
        maxlength="48"
        autocomplete="off"
        spellcheck="false"
      />
    </div>

    <div class="flex flex-col gap-1">
      <span class="text-xs text-secondary">{$t('cosmetics.model')}</span>
      <SegmentedControl
        options={[
          { value: 'classic', label: $t('cosmetics.modelClassic') },
          { value: 'slim', label: $t('cosmetics.modelSlim') },
        ]}
        value={variant}
        onChange={(v) => (variant = v as SkinVariant)}
        variant="inline"
        ariaLabel={$t('cosmetics.model')}
      />
    </div>

    {#if capes !== null}
      <div class="flex flex-col gap-1">
        <span class="text-xs text-secondary" id="skin-lib-cape-label">
          {$t('skinLibrary.capeLabel')}
        </span>
        <Select
          value={capeId ?? NONE}
          options={capeOptions}
          onChange={(v) => (capeId = v === NONE ? null : String(v))}
          ariaLabel={$t('skinLibrary.capeLabel')}
        />
      </div>
    {/if}

    {#if error}
      <p class="text-sm text-danger" role="alert">{error}</p>
    {/if}

    <div class="flex justify-end gap-2 mt-2">
      <button type="button" class="btn-secondary btn-sm" onclick={onCancel} disabled={busy}>
        {$t('common.cancel')}
      </button>
      <button type="submit" class="btn-primary btn-sm" disabled={!canSubmit}>
        {$t('skinLibrary.save')}
      </button>
    </div>
  </form>
</Modal>
