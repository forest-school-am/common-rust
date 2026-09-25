// The TEST transport for the common-oidc browser shim, shipped WITH the crate
// (the SHIM_DTS pattern, R117): this file is the SINGLE source and each SPA
// consumer writes it verbatim, so no repo hand-rolls its own.
//
// WHY A STUB EXISTS AT ALL. The served shim (/common-oidc.js) installs its
// re-auth guard AT MODULE LOAD: it wraps window.fetch for every later caller,
// reads a `#config` element a test page does not have, and on a flagged 401
// calls location.assign, which jsdom refuses outright ("Not implemented:
// navigation"). A SPA driving the generated client under vitest therefore
// cannot import the real module — and the test runner sees neither the build's
// `external` nor the tsconfig `paths` mapping that make the served specifier
// resolve anywhere.
//
// WHAT IT KEEPS. The transport below is byte-identical to the shim's, asserted
// by a test in this crate, so tests exercise the REAL query building, the
// FormData passthrough, the 204 case and the CallFailure shape. Only the guard
// differs: a no-op, and — the point — not run at load, so `fetch` stays
// whatever the test installed.
//
// Wire it up in vitest.config.ts:
//
//   resolve: { alias: { "/common-oidc.js": resolve(__dirname, "<this file>") } }
//
// The alias must be the exact specifier the generated client imports.

/**
 * A no-op. Tests install their own `fetch`, and the real guard's navigation
 * cannot run under jsdom. Exported so the stub's surface matches the shim's.
 */
export function installReauthGuard(): void {}

// >>> transport — byte-identical with common-oidc.test-stub.ts, asserted by a
// test. The stub exists because this module installs its guard at load; the
// transport itself is the same code in both, so it must not drift.
export class CallFailure extends Error {
  declare status: number;
  declare body: unknown;
  constructor(status: number, body: unknown) {
    super(`HTTP ${status}`);
    this.status = status;
    this.body = body;
  }
}

export async function call<T>(
  method: string,
  url: string,
  query?: object,
  body?: unknown,
): Promise<T> {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(query || {})) {
    if (v === undefined || v === null || v === "") continue;
    if (Array.isArray(v)) for (const item of v) q.append(k, String(item));
    else q.append(k, String(v));
  }
  const qs = q.toString();
  const init: RequestInit & { headers: Record<string, string> } = {
    method,
    headers: { Accept: "application/json" },
  };
  if (body instanceof FormData) init.body = body; // fetch sets the multipart boundary
  else if (body !== undefined) {
    init.headers["Content-Type"] = "application/json";
    init.body = JSON.stringify(body);
  }
  const resp = await fetch(qs ? `${url}?${qs}` : url, init);
  if (!resp.ok) throw new CallFailure(resp.status, await resp.json().catch(() => undefined));
  return resp.status === 204 ? (undefined as T) : resp.json();
}
// <<< transport
