# common-ui-build

The build-time glue a common-ui consumer's `build.rs` calls (R117,
crate-carried delivery). Since R117 there is **no Garage fetch, no manifest and
no prefix pin**: the assets are carried in `common-ui-core` (core: base/elements
/js/shell/d.ts + SRIs + CSP) and `common-theme` (palettes + loader), both as
committed consts. This crate just stamps and hands those to the build.

- **Version:** `0.3.0` (workspace).
- Member of the `common-rust` workspace (`crates/ui-build`).

## Depend on it

```toml
[build-dependencies]
common-ui-build = { workspace = true }   # or path = "../common-rust/crates/ui-build"
```

The pin is now the **Cargo version** of `common-ui-core` / `common-theme` — a
bump is a version bump on those crates (bytes re-emitted by `common-ui-e`'s
`build.sh`), not a `prefix` line. There is no `[package.metadata.common-ui]`.

## What a build.rs does

```rust
let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());

let mut values = common_ui_build::shared_markers();   // prefix + the four SRIs
values.push(("title", title));                        // app-specific [[markers]]
// … the rest of the shell's build markers …
let refs: Vec<(&str,&str)> = values.iter().map(|(k,v)| (*k, v.as_str())).collect();

let html = common_ui_build::stamp(common_ui_core::SHELL_HTML, &refs);
let csp  = common_ui_build::csp();          // policy, {{assets_origin}} still in it
common_ui_build::write_dts(&out).unwrap();  // common-ui.d.ts for the typecheck
```

- `shared_markers()` — the build markers identical in every consumer: `prefix`
  (the `common-ui-core` version, printed in the footer) and `sri_base_css`,
  `sri_palette_css` (from `common-theme`), `sri_elements_css`, `sri_common_ui_js`.
  Read straight from the crates, so a hash is never hand-copied.
- `stamp(shell, values)` — fills the `[[build markers]]` via `upon`, ERRORS on
  any unfilled/misspelled one, and leaves the `{{runtime markers}}`
  (`assets_origin`, `config`) for `common_templating`'s per-request render.
- `write_dts(out_dir)` — writes `common_ui_core::COMMON_UI_DTS` for tsconfig
  `paths`; the consumer keeps no vendored copy.
- `csp()` — `common_ui_core::CSP`.

## Serving the assets

The consumer serves the bytes itself at `/assets/...` (via `common_routing`'s
static-file mechanism): `common_ui_core::{BASE_CSS, ELEMENTS_CSS, COMMON_UI_JS}`,
`common_theme::{PALETTES, LOADER_JS}`, and each palette from `common_theme::palette(name)`.
The shell links `/assets/base.css`, `/assets/theme-default/palette.css`,
`/assets/theme-loader.js`, `/assets/elements.css`, `/assets/common-ui.js`.

## Tests

`cargo test -p common-ui-build`: `stamp` round-trips the real
`common_ui_core::SHELL_HTML`, and `shared_markers` matches the crate consts. No
network, no fixtures.
