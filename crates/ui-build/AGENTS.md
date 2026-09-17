# AGENTS.md — common-ui-build

## Purpose
The build-time glue for common-ui consumers (R117, crate-carried delivery). The
assets are carried in `common-ui-core` and `common-theme` as committed consts
(emitted by `common-ui-e`'s `build.sh`); this crate stamps the shell and hands
a `build.rs` the shared markers, the CSP and the d.ts. No Garage fetch, no
manifest, no prefix pin, no cache — all retired with R117.

## Layout
- `src/lib.rs` — `shared_markers`, `stamp`, `write_dts`, `csp`, `TYPES`.

## Invariants
- THE PIN IS THE CARGO VERSION of `common-ui-core` / `common-theme`. No
  `[package.metadata.common-ui] prefix`, no lock, no vendor dir, no fetch. A
  bump is a version bump on those crates (bytes re-emitted by common-ui-e).
- SRIs ARE READ FROM THE CRATES, never hand-copied: `shared_markers()` pulls
  `sri_base_css`/`sri_elements_css`/`sri_common_ui_js` from `common_ui_core` and
  `sri_palette_css` from `common_theme::SRI_DEFAULT_PALETTE` (the default palette
  lives in common-theme). `prefix` is `common-ui <VERSION>`, for the footer.
- `stamp` FILLS `[[build markers]]` VIA `upon`, erroring on any unfilled or
  misspelled one, and LEAVES `{{runtime markers}}` (`assets_origin`, `config`)
  for common-templating's render. It does not escape: values are the consumer's
  own constants and the crates' SRIs.
- NO MARKER-SET CHECK (user, R114.2). The app's boot renders the stamped shell
  through `common_templating::Shell`; upon refuses an unfilled `{{marker}}` by
  name. `shell-markers.json`'s arrays are not parsed here.
- NO SERVER-SIDE THEME LOGIC. The theme override link is gone; the browser
  loader (`common_theme::LOADER_JS`, served at `/assets/theme-loader.js`) reads
  the single `les_theme` cookie and swaps the default palette `<link>`. The
  charset validation is in the loader (the security boundary), not in Rust.
- THE CONSUMER SERVES THE BYTES at `/assets/...` from `common_ui_core` and
  `common_theme` via `common_routing`'s static-file mechanism.
