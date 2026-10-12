import { beforeAll, describe, expect, it } from 'vitest';
import { locale } from '$lib/i18n';
import type { Error as IpcError, NetworkFailure } from '$lib/ipc/bindings';
import { formatError } from '$lib/ipc/format-error';
import { hostOf } from '$lib/ipc/network-failure';

const URL_ = 'https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml';
const HOST = 'maven.minecraftforge.net';
const DETAILS = 'error sending request for url (RAW): client error (Connect): RAW_CHAIN';

const FAILURES: [string, NetworkFailure][] = [
  ['unreachable', { kind: 'unreachable' }],
  ['timeout', { kind: 'timeout' }],
  ['tls', { kind: 'tls' }],
  ['interrupted', { kind: 'interrupted' }],
  ['429', { kind: 'http_status', status: 429 }],
  ['404', { kind: 'http_status', status: 404 }],
  ['503', { kind: 'http_status', status: 503 }],
  ['403', { kind: 'http_status', status: 403 }],
  ['302', { kind: 'http_status', status: 302 }],
  ['bad_response', { kind: 'bad_response' }],
  ['other', { kind: 'other' }],
];

function both(failure: NetworkFailure): IpcError[] {
  return [
    { kind: 'network', url: URL_, failure, details: DETAILS },
    { kind: 'mods_network', url: URL_, failure, details: DETAILS },
  ];
}

describe('network failures say which site failed and why', () => {
  for (const lang of ['en', 'ru'] as const) {
    describe(lang, () => {
      beforeAll(() => locale.set(lang));

      for (const [label, failure] of FAILURES) {
        it(`${label}: names the host, never the URL or the transport detail`, () => {
          for (const e of both(failure)) {
            const msg = formatError(e);
            expect(msg).toContain(HOST);
            expect(msg).not.toContain(URL_);
            expect(msg).not.toContain('RAW');
            expect(msg).not.toContain('errors.');
            expect(msg).not.toContain('{');
          }
        });
      }

      it('points at Logs only when the cause is not recognised', () => {
        const logs = lang === 'en' ? 'Logs' : '«Логах»';
        for (const [label, failure] of FAILURES) {
          for (const e of both(failure)) {
            expect({ label, logs: formatError(e).includes(logs) }).toEqual({
              label,
              logs: label === 'other',
            });
          }
        }
      });

      it('says the status for a site error, and not "check your internet"', () => {
        const msg = formatError(both({ kind: 'http_status', status: 503 })[0]);
        expect(msg).toContain('503');
        expect(msg.toLowerCase()).not.toContain(lang === 'en' ? 'internet' : 'интернет');
      });
    });
  }

  it('does not send the user to their internet connection for a server on this computer', () => {
    locale.set('en');
    const msg = formatError({
      kind: 'network',
      url: 'http://127.0.0.1:11434/v1/chat/completions',
      failure: { kind: 'unreachable' },
      details: DETAILS,
    });
    expect(msg).toContain('127.0.0.1');
    expect(msg).toContain('local server');
    expect(msg.toLowerCase()).not.toContain('internet');
  });

  it('words every failure differently', () => {
    locale.set('en');
    const msgs = FAILURES.map(([, f]) => formatError(both(f)[0]));
    expect(new Set(msgs).size).toBe(msgs.length);
  });
});

describe('hostOf', () => {
  it('keeps only the host of a URL', () => {
    expect(hostOf('https://api.modrinth.com/v2/search?query=x')).toBe('api.modrinth.com');
    expect(hostOf('http://127.0.0.1:8080/x')).toBe('127.0.0.1');
  });

  it('shows a string that is not a URL as it is rather than hiding it', () => {
    expect(hostOf('not a url')).toBe('not a url');
  });
});
