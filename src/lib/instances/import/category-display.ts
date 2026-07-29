import type { TranslationKey } from '$lib/i18n/keys.generated';
import type { ContentCategory } from '$lib/ipc/bindings';

/** i18n key for a copyable content category's display label. Shared by the
 *  launcher-import dialog, the clone dialog, and the operations view. */
export function categoryLabelKey(cat: ContentCategory): TranslationKey {
  switch (cat) {
    case 'mods':
      return 'instances.import.categoryMods';
    case 'config':
      return 'instances.import.categoryConfig';
    case 'saves':
      return 'instances.import.categorySaves';
    case 'resource_packs':
      return 'instances.import.categoryResourcePacks';
    case 'shaderpacks':
      return 'instances.import.categoryShaderpacks';
    case 'options_txt':
      return 'instances.import.categoryOptionsTxt';
  }
}
