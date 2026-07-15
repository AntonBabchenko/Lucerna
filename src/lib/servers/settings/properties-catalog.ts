// src/lib/servers/settings/properties-catalog.ts

export type PropType =
  | { kind: 'bool' }
  | { kind: 'enum'; values: string[] }
  | { kind: 'int'; min?: number; max?: number }
  | { kind: 'string'; secret?: boolean };

export type PropGroup =
  | 'gameplay'
  | 'world'
  | 'players'
  | 'performance'
  | 'network'
  | 'rcon'
  | 'resourcePack'
  | 'misc';

export interface PropSpec {
  key: string;
  type: PropType;
  default: string;
  group: PropGroup;
}

/** Group render order. The "other" pseudo-group is appended by the UI. */
export const PROP_GROUP_ORDER: PropGroup[] = [
  'gameplay',
  'world',
  'players',
  'performance',
  'network',
  'rcon',
  'resourcePack',
  'misc',
];

/** Derive the i18n leaf key: strip '-'/'.' separators, camelCase. */
export function keyToI18n(key: string): string {
  return key.replace(/[.-](\w)/g, (_m, c: string) => c.toUpperCase());
}

const bool = (): PropType => ({ kind: 'bool' });
const int = (min?: number, max?: number): PropType => ({ kind: 'int', min, max });
const str = (secret = false): PropType => ({ kind: 'string', secret });

export const PROPERTIES_CATALOG: PropSpec[] = [
  // ── Gameplay ──────────────────────────────────────────────────────────────
  {
    key: 'gamemode',
    type: { kind: 'enum', values: ['survival', 'creative', 'adventure', 'spectator'] },
    default: 'survival',
    group: 'gameplay',
  },
  {
    key: 'difficulty',
    type: { kind: 'enum', values: ['peaceful', 'easy', 'normal', 'hard'] },
    default: 'easy',
    group: 'gameplay',
  },
  { key: 'hardcore', type: bool(), default: 'false', group: 'gameplay' },
  { key: 'pvp', type: bool(), default: 'true', group: 'gameplay' },
  { key: 'force-gamemode', type: bool(), default: 'false', group: 'gameplay' },
  { key: 'allow-flight', type: bool(), default: 'false', group: 'gameplay' },
  { key: 'allow-nether', type: bool(), default: 'true', group: 'gameplay' },
  { key: 'enable-command-block', type: bool(), default: 'false', group: 'gameplay' },
  { key: 'spawn-monsters', type: bool(), default: 'true', group: 'gameplay' },
  { key: 'spawn-animals', type: bool(), default: 'true', group: 'gameplay' },
  { key: 'spawn-npcs', type: bool(), default: 'true', group: 'gameplay' },
  { key: 'generate-structures', type: bool(), default: 'true', group: 'gameplay' },
  { key: 'player-idle-timeout', type: int(0), default: '0', group: 'gameplay' },
  { key: 'op-permission-level', type: int(0, 4), default: '4', group: 'gameplay' },
  { key: 'function-permission-level', type: int(1, 4), default: '2', group: 'gameplay' },
  // ── World & generation ────────────────────────────────────────────────────
  { key: 'level-name', type: str(), default: 'world', group: 'world' },
  { key: 'level-seed', type: str(), default: '', group: 'world' },
  { key: 'level-type', type: str(), default: 'minecraft:normal', group: 'world' },
  { key: 'generator-settings', type: str(), default: '{}', group: 'world' },
  { key: 'max-world-size', type: int(1, 29999984), default: '29999984', group: 'world' },
  { key: 'spawn-protection', type: int(0), default: '16', group: 'world' },
  { key: 'initial-enabled-packs', type: str(), default: 'vanilla', group: 'world' },
  { key: 'initial-disabled-packs', type: str(), default: '', group: 'world' },
  // ── Players & access ──────────────────────────────────────────────────────
  { key: 'max-players', type: int(0), default: '20', group: 'players' },
  { key: 'white-list', type: bool(), default: 'false', group: 'players' },
  { key: 'enforce-whitelist', type: bool(), default: 'false', group: 'players' },
  { key: 'online-mode', type: bool(), default: 'true', group: 'players' },
  { key: 'enforce-secure-profile', type: bool(), default: 'true', group: 'players' },
  { key: 'motd', type: str(), default: 'A Minecraft Server', group: 'players' },
  { key: 'hide-online-players', type: bool(), default: 'false', group: 'players' },
  // ── View / simulation / performance ───────────────────────────────────────
  { key: 'view-distance', type: int(2, 32), default: '10', group: 'performance' },
  { key: 'simulation-distance', type: int(2, 32), default: '10', group: 'performance' },
  {
    key: 'entity-broadcast-range-percentage',
    type: int(10, 1000),
    default: '100',
    group: 'performance',
  },
  { key: 'max-tick-time', type: int(-1), default: '60000', group: 'performance' },
  { key: 'sync-chunk-writes', type: bool(), default: 'true', group: 'performance' },
  { key: 'max-chained-neighbor-updates', type: int(), default: '1000000', group: 'performance' },
  { key: 'network-compression-threshold', type: int(-1), default: '256', group: 'performance' },
  { key: 'pause-when-empty-seconds', type: int(0), default: '60', group: 'performance' },
  // ── Network ───────────────────────────────────────────────────────────────
  { key: 'server-ip', type: str(), default: '', group: 'network' },
  { key: 'server-port', type: int(1, 65535), default: '25565', group: 'network' },
  { key: 'prevent-proxy-connections', type: bool(), default: 'false', group: 'network' },
  { key: 'rate-limit', type: int(0), default: '0', group: 'network' },
  { key: 'use-native-transport', type: bool(), default: 'true', group: 'network' },
  { key: 'enable-status', type: bool(), default: 'true', group: 'network' },
  { key: 'accepts-transfers', type: bool(), default: 'false', group: 'network' },
  // ── RCON & Query ──────────────────────────────────────────────────────────
  { key: 'enable-rcon', type: bool(), default: 'false', group: 'rcon' },
  { key: 'rcon.port', type: int(1, 65535), default: '25575', group: 'rcon' },
  { key: 'rcon.password', type: str(true), default: '', group: 'rcon' },
  { key: 'broadcast-rcon-to-ops', type: bool(), default: 'true', group: 'rcon' },
  { key: 'enable-query', type: bool(), default: 'false', group: 'rcon' },
  { key: 'query.port', type: int(1, 65535), default: '25565', group: 'rcon' },
  // ── Resource pack ─────────────────────────────────────────────────────────
  { key: 'resource-pack', type: str(), default: '', group: 'resourcePack' },
  { key: 'resource-pack-sha1', type: str(), default: '', group: 'resourcePack' },
  { key: 'resource-pack-id', type: str(), default: '', group: 'resourcePack' },
  { key: 'resource-pack-prompt', type: str(), default: '', group: 'resourcePack' },
  { key: 'require-resource-pack', type: bool(), default: 'false', group: 'resourcePack' },
  // ── Advanced / misc ───────────────────────────────────────────────────────
  { key: 'enable-jmx-monitoring', type: bool(), default: 'false', group: 'misc' },
  { key: 'broadcast-console-to-ops', type: bool(), default: 'true', group: 'misc' },
  { key: 'text-filtering-config', type: str(), default: '', group: 'misc' },
  { key: 'log-ips', type: bool(), default: 'true', group: 'misc' },
  { key: 'bug-report-link', type: str(), default: '', group: 'misc' },
];
