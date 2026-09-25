// The backend answers an expired session's XHR with 401 + this header. Nothing
// else distinguishes it from an ordinary 401, which a page may handle itself.
const REAUTH_HEADER = "x-common-oidc-reauth";
const LOOP_GUARD_KEY = "common_oidc_last_bounce";
const LOOP_GUARD_MS = 10_000;

interface Config {
  loginPath: string;
}

let bouncing = false;

function loginPath(): string | null {
  const block = document.getElementById("config");
  if (!block?.textContent) return null;
  try {
    const config = JSON.parse(block.textContent) as Partial<Config>;
    return typeof config.loginPath === "string" && config.loginPath ? config.loginPath : null;
  } catch {
    return null;
  }
}

function recentlyBounced(): boolean {
  try {
    const last = Number(sessionStorage.getItem(LOOP_GUARD_KEY) ?? 0);
    return Date.now() - last < LOOP_GUARD_MS;
  } catch {
    return false;
  }
}

// prompt=none needs a real top-level navigation to authentik, whose SSO cookie
// lives on its own origin, so this leaves the page; on success the browser
// lands back on `next`. The winner gets a promise that never resolves because
// the page is going away.
function bounce(path: string): Promise<void> {
  if (bouncing || recentlyBounced()) return Promise.resolve();
  bouncing = true;
  try {
    sessionStorage.setItem(LOOP_GUARD_KEY, String(Date.now()));
  } catch {
    // private mode: the server-side one-shot interactive escalation is the net
  }
  const next = location.pathname + location.search + location.hash;
  location.assign(`${path}?next=${encodeURIComponent(next)}`);
  return new Promise<void>(() => {});
}

export function installReauthGuard(): void {
  const flagged = window as unknown as { __commonOidcGuarded?: boolean };
  if (flagged.__commonOidcGuarded) return;
  flagged.__commonOidcGuarded = true;

  const original = window.fetch.bind(window);
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const response = await original(input, init);
    if (response.status === 401 && response.headers.get(REAUTH_HEADER)) {
      const path = loginPath();
      if (path) await bounce(path);
    }
    return response;
  };
}

installReauthGuard();

// The transport under every app's generated client (`bindings/client.ts`):
// `call(method, url, query?, body?)`. It lives HERE, beside the auth logic it
// leans on, rather than vendored per app. The 401 → re-auth is not duplicated:
// `call` issues its request through the GLOBAL `fetch`, which `installReauthGuard`
// (run above at module load) has already wrapped, so an expired session's 401
// bounces to login through the same guard every other request goes through.
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
