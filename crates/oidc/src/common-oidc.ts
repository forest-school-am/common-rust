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
