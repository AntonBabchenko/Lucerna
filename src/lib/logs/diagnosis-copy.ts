import type { TranslationKey } from '$lib/i18n/keys.generated';

type DiagnosisCopyKeys = {
  title: TranslationKey;
  explanation: TranslationKey;
  recommendation: TranslationKey;
};

/**
 * Maps a backend diagnosis `pattern_id` to its localized i18n keys. The Rust
 * diagnoser ships English `title`/`explanation`/`recommendation`; the UI
 * prefers these localized keys and falls back to the backend English string
 * for any pattern not listed here.
 */
export const DIAGNOSIS_COPY: Record<string, DiagnosisCopyKeys> = {
  'java-version-too-old': {
    title: 'logs.diagnosis.patterns.javaVersionTooOld.title',
    explanation: 'logs.diagnosis.patterns.javaVersionTooOld.explanation',
    recommendation: 'logs.diagnosis.patterns.javaVersionTooOld.recommendation',
  },
  'mod-resolution-conflict': {
    title: 'logs.diagnosis.patterns.modResolutionConflict.title',
    explanation: 'logs.diagnosis.patterns.modResolutionConflict.explanation',
    recommendation: 'logs.diagnosis.patterns.modResolutionConflict.recommendation',
  },
  'fabric-loader-missing-main': {
    title: 'logs.diagnosis.patterns.fabricLoaderMissingMain.title',
    explanation: 'logs.diagnosis.patterns.fabricLoaderMissingMain.explanation',
    recommendation: 'logs.diagnosis.patterns.fabricLoaderMissingMain.recommendation',
  },
  'corrupt-mod-jar': {
    title: 'logs.diagnosis.patterns.corruptModJar.title',
    explanation: 'logs.diagnosis.patterns.corruptModJar.explanation',
    recommendation: 'logs.diagnosis.patterns.corruptModJar.recommendation',
  },
  'out-of-memory': {
    title: 'logs.diagnosis.patterns.outOfMemory.title',
    explanation: 'logs.diagnosis.patterns.outOfMemory.explanation',
    recommendation: 'logs.diagnosis.patterns.outOfMemory.recommendation',
  },
  'port-already-in-use': {
    title: 'logs.diagnosis.patterns.portAlreadyInUse.title',
    explanation: 'logs.diagnosis.patterns.portAlreadyInUse.explanation',
    recommendation: 'logs.diagnosis.patterns.portAlreadyInUse.recommendation',
  },
  'disk-full': {
    title: 'logs.diagnosis.patterns.diskFull.title',
    explanation: 'logs.diagnosis.patterns.diskFull.explanation',
    recommendation: 'logs.diagnosis.patterns.diskFull.recommendation',
  },
  'server-missing-mods': {
    title: 'logs.diagnosis.patterns.serverMissingMods.title',
    explanation: 'logs.diagnosis.patterns.serverMissingMods.explanation',
    recommendation: 'logs.diagnosis.patterns.serverMissingMods.recommendation',
  },
  'client-extra-mods': {
    title: 'logs.diagnosis.patterns.clientExtraMods.title',
    explanation: 'logs.diagnosis.patterns.clientExtraMods.explanation',
    recommendation: 'logs.diagnosis.patterns.clientExtraMods.recommendation',
  },
};
