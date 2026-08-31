# Parked design notes (not implemented)

Recorded for the future; deliberately NOT built (DECISIONS.md / manager
deprioritization 2026-08-31). Do not let these gate the crate.

## Iframe/portal support (parked — portal is design-thinking only)

If a hub app ever iframes the other stand apps (same-site `*.dev.local`, so
Lax cookies + silent redirects flow in frames), the crate would need:

1. **`frame-ancestors` config knob** — embeddable apps set
   `Content-Security-Policy: frame-ancestors 'self' https://role-ui.dev.local`
   (or a stand-wide `*.dev.local` policy). One crate knob beats each adopter
   hand-rolling the header. Decide the default (`'self'` vs stand-wide) then.
2. **Iframe-aware interactive fallback** — silent `prompt=none` re-auth is
   redirect-only and works framed, but a truly-dead session's *interactive*
   login must not render inside a frame (authentik will likely refuse). Detect
   a framed context and escalate via a top-level navigation (a minimal
   "continue sign-in" interstitial targeting `_top`), then land back in the
   app.

Both are policy-layer only (no protocol impact) and orthogonal to the refresh
flag. Ship as a point release if the portal is ever built (as a separate
standalone service, per the current ruling — not part of role-ui).
