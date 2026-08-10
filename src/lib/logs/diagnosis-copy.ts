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
  'create-goggle-overlay-crash': {
    title: 'logs.diagnosis.patterns.createGoggleOverlayCrash.title',
    explanation: 'logs.diagnosis.patterns.createGoggleOverlayCrash.explanation',
    recommendation: 'logs.diagnosis.patterns.createGoggleOverlayCrash.recommendation',
  },
  'javafml-language-provider': {
    title: 'logs.diagnosis.patterns.javafmlLanguageProvider.title',
    explanation: 'logs.diagnosis.patterns.javafmlLanguageProvider.explanation',
    recommendation: 'logs.diagnosis.patterns.javafmlLanguageProvider.recommendation',
  },
  'forge-duplicate-mods': {
    title: 'logs.diagnosis.patterns.forgeDuplicateMods.title',
    explanation: 'logs.diagnosis.patterns.forgeDuplicateMods.explanation',
    recommendation: 'logs.diagnosis.patterns.forgeDuplicateMods.recommendation',
  },
  'forge-missing-dependencies': {
    title: 'logs.diagnosis.patterns.forgeMissingDependencies.title',
    explanation: 'logs.diagnosis.patterns.forgeMissingDependencies.explanation',
    recommendation: 'logs.diagnosis.patterns.forgeMissingDependencies.recommendation',
  },
  'fabric-unmet-dependency': {
    title: 'logs.diagnosis.patterns.fabricUnmetDependency.title',
    explanation: 'logs.diagnosis.patterns.fabricUnmetDependency.explanation',
    recommendation: 'logs.diagnosis.patterns.fabricUnmetDependency.recommendation',
  },
  'mixin-apply-failed': {
    title: 'logs.diagnosis.patterns.mixinApplyFailed.title',
    explanation: 'logs.diagnosis.patterns.mixinApplyFailed.explanation',
    recommendation: 'logs.diagnosis.patterns.mixinApplyFailed.recommendation',
  },
  'broken-mod-metadata': {
    title: 'logs.diagnosis.patterns.brokenModMetadata.title',
    explanation: 'logs.diagnosis.patterns.brokenModMetadata.explanation',
    recommendation: 'logs.diagnosis.patterns.brokenModMetadata.recommendation',
  },
  'overlay-render-crash': {
    title: 'logs.diagnosis.patterns.overlayRenderCrash.title',
    explanation: 'logs.diagnosis.patterns.overlayRenderCrash.explanation',
    recommendation: 'logs.diagnosis.patterns.overlayRenderCrash.recommendation',
  },
  'datapack-load-failed': {
    title: 'logs.diagnosis.patterns.datapackLoadFailed.title',
    explanation: 'logs.diagnosis.patterns.datapackLoadFailed.explanation',
    recommendation: 'logs.diagnosis.patterns.datapackLoadFailed.recommendation',
  },
  'mod-linkage-error': {
    title: 'logs.diagnosis.patterns.modLinkageError.title',
    explanation: 'logs.diagnosis.patterns.modLinkageError.explanation',
    recommendation: 'logs.diagnosis.patterns.modLinkageError.recommendation',
  },
  'oom-metaspace': {
    title: 'logs.diagnosis.patterns.oomMetaspace.title',
    explanation: 'logs.diagnosis.patterns.oomMetaspace.explanation',
    recommendation: 'logs.diagnosis.patterns.oomMetaspace.recommendation',
  },
  'oom-gc-overhead': {
    title: 'logs.diagnosis.patterns.oomGcOverhead.title',
    explanation: 'logs.diagnosis.patterns.oomGcOverhead.explanation',
    recommendation: 'logs.diagnosis.patterns.oomGcOverhead.recommendation',
  },
  'heap-reserve-failed': {
    title: 'logs.diagnosis.patterns.heapReserveFailed.title',
    explanation: 'logs.diagnosis.patterns.heapReserveFailed.explanation',
    recommendation: 'logs.diagnosis.patterns.heapReserveFailed.recommendation',
  },
  'bad-jvm-arg': {
    title: 'logs.diagnosis.patterns.badJvmArg.title',
    explanation: 'logs.diagnosis.patterns.badJvmArg.explanation',
    recommendation: 'logs.diagnosis.patterns.badJvmArg.recommendation',
  },
  'natives-link-error': {
    title: 'logs.diagnosis.patterns.nativesLinkError.title',
    explanation: 'logs.diagnosis.patterns.nativesLinkError.explanation',
    recommendation: 'logs.diagnosis.patterns.nativesLinkError.recommendation',
  },
  'jvm-native-crash': {
    title: 'logs.diagnosis.patterns.jvmNativeCrash.title',
    explanation: 'logs.diagnosis.patterns.jvmNativeCrash.explanation',
    recommendation: 'logs.diagnosis.patterns.jvmNativeCrash.recommendation',
  },
  'glfw-no-opengl': {
    title: 'logs.diagnosis.patterns.glfwNoOpengl.title',
    explanation: 'logs.diagnosis.patterns.glfwNoOpengl.explanation',
    recommendation: 'logs.diagnosis.patterns.glfwNoOpengl.recommendation',
  },
  'pixel-format-not-accelerated': {
    title: 'logs.diagnosis.patterns.pixelFormatNotAccelerated.title',
    explanation: 'logs.diagnosis.patterns.pixelFormatNotAccelerated.explanation',
    recommendation: 'logs.diagnosis.patterns.pixelFormatNotAccelerated.recommendation',
  },
  'opengl-error-spam': {
    title: 'logs.diagnosis.patterns.openglErrorSpam.title',
    explanation: 'logs.diagnosis.patterns.openglErrorSpam.explanation',
    recommendation: 'logs.diagnosis.patterns.openglErrorSpam.recommendation',
  },
  'invalid-session': {
    title: 'logs.diagnosis.patterns.invalidSession.title',
    explanation: 'logs.diagnosis.patterns.invalidSession.explanation',
    recommendation: 'logs.diagnosis.patterns.invalidSession.recommendation',
  },
  'not-whitelisted': {
    title: 'logs.diagnosis.patterns.notWhitelisted.title',
    explanation: 'logs.diagnosis.patterns.notWhitelisted.explanation',
    recommendation: 'logs.diagnosis.patterns.notWhitelisted.recommendation',
  },
  'dns-unknown-host': {
    title: 'logs.diagnosis.patterns.dnsUnknownHost.title',
    explanation: 'logs.diagnosis.patterns.dnsUnknownHost.explanation',
    recommendation: 'logs.diagnosis.patterns.dnsUnknownHost.recommendation',
  },
  'ssl-handshake-failed': {
    title: 'logs.diagnosis.patterns.sslHandshakeFailed.title',
    explanation: 'logs.diagnosis.patterns.sslHandshakeFailed.explanation',
    recommendation: 'logs.diagnosis.patterns.sslHandshakeFailed.recommendation',
  },
  'connection-timed-out': {
    title: 'logs.diagnosis.patterns.connectionTimedOut.title',
    explanation: 'logs.diagnosis.patterns.connectionTimedOut.explanation',
    recommendation: 'logs.diagnosis.patterns.connectionTimedOut.recommendation',
  },
  'connection-refused': {
    title: 'logs.diagnosis.patterns.connectionRefused.title',
    explanation: 'logs.diagnosis.patterns.connectionRefused.explanation',
    recommendation: 'logs.diagnosis.patterns.connectionRefused.recommendation',
  },
  'session-lock': {
    title: 'logs.diagnosis.patterns.sessionLock.title',
    explanation: 'logs.diagnosis.patterns.sessionLock.explanation',
    recommendation: 'logs.diagnosis.patterns.sessionLock.recommendation',
  },
  'chunk-corruption': {
    title: 'logs.diagnosis.patterns.chunkCorruption.title',
    explanation: 'logs.diagnosis.patterns.chunkCorruption.explanation',
    recommendation: 'logs.diagnosis.patterns.chunkCorruption.recommendation',
  },
  'level-dat-corrupt': {
    title: 'logs.diagnosis.patterns.levelDatCorrupt.title',
    explanation: 'logs.diagnosis.patterns.levelDatCorrupt.explanation',
    recommendation: 'logs.diagnosis.patterns.levelDatCorrupt.recommendation',
  },
  'access-denied': {
    title: 'logs.diagnosis.patterns.accessDenied.title',
    explanation: 'logs.diagnosis.patterns.accessDenied.explanation',
    recommendation: 'logs.diagnosis.patterns.accessDenied.recommendation',
  },
  'shader-compile-failed': {
    title: 'logs.diagnosis.patterns.shaderCompileFailed.title',
    explanation: 'logs.diagnosis.patterns.shaderCompileFailed.explanation',
    recommendation: 'logs.diagnosis.patterns.shaderCompileFailed.recommendation',
  },
  'server-eula-not-accepted': {
    title: 'logs.diagnosis.patterns.serverEulaNotAccepted.title',
    explanation: 'logs.diagnosis.patterns.serverEulaNotAccepted.explanation',
    recommendation: 'logs.diagnosis.patterns.serverEulaNotAccepted.recommendation',
  },
  'server-port-in-use': {
    title: 'logs.diagnosis.patterns.serverPortInUse.title',
    explanation: 'logs.diagnosis.patterns.serverPortInUse.explanation',
    recommendation: 'logs.diagnosis.patterns.serverPortInUse.recommendation',
  },
  'server-out-of-memory': {
    title: 'logs.diagnosis.patterns.serverOutOfMemory.title',
    explanation: 'logs.diagnosis.patterns.serverOutOfMemory.explanation',
    recommendation: 'logs.diagnosis.patterns.serverOutOfMemory.recommendation',
  },
  'server-cant-keep-up': {
    title: 'logs.diagnosis.patterns.serverCantKeepUp.title',
    explanation: 'logs.diagnosis.patterns.serverCantKeepUp.explanation',
    recommendation: 'logs.diagnosis.patterns.serverCantKeepUp.recommendation',
  },
  'server-watchdog-stall': {
    title: 'logs.diagnosis.patterns.serverWatchdogStall.title',
    explanation: 'logs.diagnosis.patterns.serverWatchdogStall.explanation',
    recommendation: 'logs.diagnosis.patterns.serverWatchdogStall.recommendation',
  },
};
