// stand-oidc browser shim — served by the app's own backend at
// GET /stand-oidc.js with the login path baked in, so a frontend can never
// version-skew against the backend's 401 contract (the searchbase.js
// precedent). Framework-free; import as a module.
//
// The contract (DECISIONS.md R2, Track A): the backend protects APIs with
// per-request userinfo. When the access token has expired it answers an
// XHR/fetch call with HTTP 401 and the header `X-Stand-OIDC-Reauth`. This
// shim wraps fetch: it passes every request through untouched until it sees
// that signal, then drives a silent `prompt=none` re-auth by navigating
// top-level to the login path (invisible while the SSO session is alive;
// the server escalates to interactive once if the session is truly dead).
// Guards: single-flight (N concurrent 401s → one bounce, the rest ride it)
// and a loop breaker (a bounce won't re-fire within a short window).

const LOGIN_PATH = "__LOGIN_PATH__";
const REAUTH_HEADER = "x-stand-oidc-reauth";
const LOOP_GUARD_KEY = "stand_oidc_last_bounce";
const LOOP_GUARD_MS = 10_000;

let bouncing = false; // single-flight within this document

function recentlyBounced() {
  try {
    const t = Number(sessionStorage.getItem(LOOP_GUARD_KEY) || 0);
    return Date.now() - t < LOOP_GUARD_MS;
  } catch {
    return false;
  }
}

// Navigate top-level into the silent re-auth. prompt=none needs a real
// navigation to authentik (the SSO cookie lives on its origin), so this
// leaves the page; on success the browser lands back on `next` and the app
// re-fetches naturally. Returns a never-resolving promise so awaiters simply
// stop while the navigation happens.
function bounce() {
  if (bouncing || recentlyBounced()) return Promise.resolve();
  bouncing = true;
  try {
    sessionStorage.setItem(LOOP_GUARD_KEY, String(Date.now()));
  } catch {
    /* private mode: rely on the server-side one-shot interactive escalation */
  }
  const next = location.pathname + location.search + location.hash;
  location.assign(`${LOGIN_PATH}?next=${encodeURIComponent(next)}`);
  return new Promise(() => {});
}

/// Call once at startup: wraps window.fetch so any 401 carrying the re-auth
/// signal triggers the silent bounce. Returns the original fetch.
export function installReauthGuard() {
  const original = window.fetch.bind(window);
  if (window.__standOidcGuarded) return original;
  window.__standOidcGuarded = true;
  window.fetch = async (input, init) => {
    const resp = await original(input, init);
    if (resp.status === 401 && resp.headers.get(REAUTH_HEADER)) {
      await bounce();
    }
    return resp;
  };
  return original;
}

/// Explicit one-off for callers that don't want global fetch patched:
/// `const r = await standFetch(url, opts)` bounces on the signal.
export async function standFetch(input, init) {
  const resp = await fetch(input, init);
  if (resp.status === 401 && resp.headers.get(REAUTH_HEADER)) {
    await bounce();
  }
  return resp;
}

/// authentik user portal — the only place sessions end (no app logout).
export const logoutIsAtAuthentik = true;

// Auto-install on import: the common case is "just protect my fetches".
installReauthGuard();
