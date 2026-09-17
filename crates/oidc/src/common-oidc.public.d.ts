// The type-only surface of the common-oidc browser shim, shipped WITH the crate
// (R117): this file is the SINGLE source. `build.rs` strips the runtime from
// src/common-oidc.ts into /common-oidc.js; the crate exposes THIS as
// `SHIM_DTS`, and each consumer writes it verbatim into its OUT_DIR for tsc —
// no vendored copy, no sync check, because the crate that serves the shim is
// the crate that declares its types.
//
// tsc reaches this through the tsconfig `paths` mapping of the served specifier
// (`/common-oidc.js`); esbuild leaves that import external. An ambient
// `declare module "/common-oidc.js"` is illegal (TS2436: a leading-slash
// specifier is a relative name), so this is a plain module — top-level exports,
// no `declare module` wrapper.

// The transport under every app's generated client (`bindings/client.ts`):
// `call(method, url, query?, body?)`.
export function call<T>(
  method: string,
  url: string,
  query?: object,
  body?: unknown,
): Promise<T>;

// Thrown by `call` on a non-2xx response; the page catches it.
export declare class CallFailure extends Error {
  status: number;
  body: unknown;
  constructor(status: number, body: unknown);
}

// Wraps the global `fetch` so an expired session's 401 (carrying the reauth
// header) bounces to login. The shim runs this once at module load; it is
// exported so a page may install the guard explicitly if it needs to.
export function installReauthGuard(): void;
