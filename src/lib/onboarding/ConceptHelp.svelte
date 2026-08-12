<script module lang="ts">
  import type { TranslationKey } from '$lib/i18n/keys.generated';

  // A "concept explainer" is a namespace in the i18n key tree whose leaves are
  // the three trigger/close strings plus one or more body paragraphs. Deriving
  // the namespace type from TranslationKey rather than accepting a bare string
  // is what makes a typo a COMPILE error instead of a raw key rendered at the
  // user: a namespace is only accepted if all three chrome leaves exist under
  // it. (Extract, not `&`: it distributes over the union; an intersection of
  // two string-literal unions collapses to never.)
  type NamespaceWith<K, Leaf extends string> = K extends `${infer Ns}.${Leaf}` ? Ns : never;

  type ConceptNamespace = Extract<
    Extract<
      NamespaceWith<TranslationKey, 'triggerAriaLabel'>,
      NamespaceWith<TranslationKey, 'triggerTitle'>
    >,
    NamespaceWith<TranslationKey, 'closeAriaLabel'>
  >;

  /** The leaves that actually exist under one concept namespace. */
  type LeafOf<K, Ns extends string> = K extends `${Ns}.${infer Leaf}` ? Leaf : never;
  type ConceptLeaf<Ns extends ConceptNamespace> = LeafOf<TranslationKey, Ns>;

  /**
   * The single `as TranslationKey` cast for this pattern — every concept
   * explainer used to carry its own copy. It is safe because both halves are
   * type-checked before they are joined: `namespace` is a real namespace and
   * `leaf` is a real leaf of it, so the composed key is in the union.
   */
  function conceptKey(namespace: ConceptNamespace, leaf: string): TranslationKey {
    return `${namespace}.${leaf}` as TranslationKey;
  }
</script>

<script lang="ts" generics="Ns extends ConceptNamespace">
  // The always-visible "(?)" concept explainer — one component for all of them.
  // It answers "what IS this thing", as opposed to a tooltip naming a control,
  // and it is the safety net for anyone who skipped (or never saw) the matching
  // tour. See docs/DESIGN.md §8.
  //
  // This replaced three near-identical wrappers that had already begun to drift
  // (two names for the same derived, three copies of the key cast). Callers name
  // the namespace and list its paragraph leaves; everything else — the level
  // swap, the key construction, the chrome strings, the width — is decided once,
  // here. `paragraphKeys` is an explicit leaf list rather than a count because
  // the list is type-checked against the namespace: `['p1', 'p5']` does not
  // compile, and `['body']` works for a one-sentence explainer whose leaf is not
  // named `pN`.
  import { t } from '$lib/i18n';
  import { explainKey } from '$lib/onboarding/explanation-keys';
  import { explanationState } from '$lib/onboarding/explanation-level.svelte';
  import HelpPopover from '$lib/ui/HelpPopover.svelte';

  let {
    namespace,
    paragraphKeys,
    width = 320,
  }: {
    /** Concept namespace, e.g. `onboarding.datapackConcept`. */
    namespace: Ns;
    /** Body leaves under `namespace`, in display order. */
    paragraphKeys: readonly ConceptLeaf<Ns>[];
    /** 320 suits a multi-paragraph panel; a one-sentence helper passes 260. */
    width?: number;
  } = $props();

  // Each paragraph goes through explainKey, which swaps in the `*Basic` sibling
  // at the Basic detail level. A parity test pins every base/`*Basic` pair in
  // both locales, so every resolved key exists.
  const paragraphs = $derived(
    paragraphKeys.map((leaf) =>
      $t(explainKey(conceptKey(namespace, leaf), explanationState.level)),
    ),
  );
</script>

<HelpPopover
  {paragraphs}
  {width}
  triggerAriaLabel={$t(conceptKey(namespace, 'triggerAriaLabel'))}
  triggerTitle={$t(conceptKey(namespace, 'triggerTitle'))}
  closeAriaLabel={$t(conceptKey(namespace, 'closeAriaLabel'))}
/>
