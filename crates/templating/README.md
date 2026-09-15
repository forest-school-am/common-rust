# common-templating

The Les stand's shared shell stamping + static-asset serving (CODESTYLE.md §9):
`upon` for the two runtime markers of a built shell, plus an mtime-keyed cache
over a validated asset directory. Depended on only by services that actually
serve assets — deliberately separate from `common-logging` (§9.7), which stays
dependency-light for every binary.

- **Version:** `0.3.0` · **Toolchain:** Rust 1.98.0.
- Member of the `common-rust` workspace (`crates/templating`), alongside
  `common-logging` and `common-oidc`.

## Depend on it

Consumer manifests declare a git dependency:

```toml
[dependencies]
common-templating = { git = "https://github.com/forest-school-am/common-rust-templating.git", tag = "v0.1.2" }
```

**That URL and tag are documentation, not a pin, and no such remote exists** —
nothing in this fleet is pushed (R21), and `common-rust` has no remote at all.
The tag predates the workspace merge; do not reason about behaviour from it.

What actually resolves the dependency is the single shared cargo patch at
`/mnt/host/workspace/Les/.cargo/config.toml` (R22a/R22b), which redirects the
URL above to this workspace. Cargo walks up from the build directory and MERGES
that file, so it already applies to every repo under `Les/`: there is nothing to
symlink, and no repo may keep a `.cargo/config.toml` of its own. You therefore
always build whatever `common-rust` currently is.

A missing or wrong path in that file does not fail — cargo silently falls back
to the published crate and rewrites your lockfile to say so. Run
`sh stand/check-cargo-patch.sh` if a build behaves oddly.

## The shell

A shell is the vendored common-ui document after a service's build.rs has
stamped every build-time marker (title, prefix, page css/module, SRI hashes,
…). What remains are exactly two RUNTIME markers, `{{assets_origin}}` and
`{{config}}`, and this crate fills them:

```rust
use common_templating::Shell;

// boot: compile once. Refuses a shell upon cannot parse.
let shell = Shell::compile(SHELL)?;

// per request: the origin raw, the config as an escaped JSON block. The
// config is any `Serialize` the service chooses — common-oidc's `PageConfig`
// on this stand.
let html = shell.render(&origin, &config)?;

// or, for a shell the build already validated, both steps in one:
let html = common_templating::render(SHELL, &origin, &config);
```

The engine is `upon` 0.11 with only its `serde` feature: no filters, no
functions, no escaping, no custom syntax. Its grammar is more than `{{name}}`,
but a shell uses nothing else. Three rules matter:

- **A missing value is a render error.** A `{{title}}` the build did not
  stamp is `RenderError::Render` whose message quotes the line, never a marker
  shipped to a browser. `render(shell, origin, config)` is total and PANICS on
  it — an unstamped marker is a build.rs defect, not a request-time condition;
  a service that wants a boot refusal instead compiles a `Shell` at boot.
- **A lone `}}` is a compile error.** upon refuses a `}}` outside an
  expression. A build-time value containing one (minified CSS or JS pasted
  into the shell, say) breaks compilation, which is why a `Shell` compiled at
  boot is the shape to prefer.
- **`{{config}}` is escaped for its block, not for HTML.** The JSON is
  serialised with `<`, `>` and `&` as `\u00XX`, so no value — a display name
  from the IdP above all — can close the `<script type="application/json">`
  it sits in or open a tag inside it, and the page still parses it back
  unchanged. `{{assets_origin}}` is inserted raw; it is `https://<host>` by
  construction (below).

What the config block SAYS is not this crate's concern: it takes any
`serde::Serialize`. The field names are a contract between the writer
(`common_oidc::PageConfig`) and the readers in common-ui, pinned there.

## Static files

```rust
use common_templating::{Builder, sha256};

// boot: validate the dir, pin server-enforced logic
let cache = Builder::new(&assets_dir)
    .pin("common-oidc.js", EXPECTED_SHA256)             // §9.8 no drift
    .build()?;                                          // refuses to boot otherwise

// §9.5a static mode — keyed by (mtime)
let logo = cache.static_file("logo.svg")?;
```

The cache invalidates on the next request after a file's mtime changes, so
editing a served file on disk takes effect without a restart. Wrap the
`AssetCache` in an `Arc` in your `AppState`.

- **Boot validation (§9.6):** the asset dir must exist and every pinned file
  must be present with the expected hash — `Builder::build` refuses
  otherwise. The asset dir is a classified config option in the host service.
- **Integrity pins (§9.7b / §9.8):** `pin(name, sha256)` verifies a file's
  content hash at boot AND on every reload, refusing to serve on mismatch —
  for logic the server also enforces (dual-use assets) and for library files
  that must not drift from the crate version. Produce the constant with
  `common_templating::sha256(bytes)`.
- **No template mode.** Files are served byte-identical; there is no
  parameterised file rendering. A service needing per-request domain data owns
  its own engine or does not server-render; pre-rendering rows to HTML strings
  in Rust to fit this API is a §9.1 violation rather than a workaround.

## The asset origin (§12.6)

The origin and its substitution into the shell's CSP come from here, so no
service composes them:

```rust
let origin = common_templating::AssetsOrigin::from_env(deployment)
    .unwrap_or_else(|r| common_logging::refuse!(r));   // your module, your log line

// The POLICY is the shell's, not this crate's: the `csp` field of the
// shell-markers.json you vendored, with {{assets_origin}} where the origin
// goes. The shell's CSP must allow this origin; a policy with no marker is
// refused rather than served.
const CSP: &str = /* build.rs: the `csp` field of your vendored shell-markers.json */;

let app = Router::new()
    .route("/", get(index))
    .layer(origin.csp_layer(CSP)?);

// in the handler, stamped into the built shell along with the config block:
let html = common_templating::render(SHELL, &origin, &config);
```

A service that renders the shell requires `ASSETS_ORIGIN` in EVERY deployment
class: `render` takes `&AssetsOrigin` rather than an `Option`, so the dev
`Ok(None)` has to become a refusal of the service's own instead of a page
naming an origin nobody chose. `ASSETS_ORIGIN_VARIABLE` is the variable's
name, for a consumer that adds a flag override.

```html
<script type="module" src="{{assets_origin}}/common-ui@<hash>/common-ui.js"
        integrity="sha384-…" crossorigin="anonymous"></script>
```

`ASSETS_ORIGIN` is `https://<host>` and nothing else — no path, port, trailing
slash, credentials, query or fragment. Anything else is a boot refusal naming
the variable, the value, what was accepted, and which rule it broke. It is
**prod-required**: unset is `None` under `DEPLOYMENT_TYPE=dev` and a refusal
under prod.

## Untrusted names (§9.5b)

Asset names are treated as untrusted input — an adopter that serves a bundle by
URL path passes a request-influenced name straight in. Every name-taking
method rejects absolute paths, `..` segments, and symlinks escaping the root
(canonicalize + containment) before any filesystem access, returning
`RenderError::UnsafeName`. The guard lives in the crate so no adopter has to
remember it.

## Scope (§9.7a)

Served content only. Embedded data never sent to a client (seed data,
fixtures, golden anchors) is out of scope and may stay embedded.

## Test

`cargo test` — both runtime markers filled everywhere + config-block escaping
(script close, ampersand, hostile display name, JSON round-trip, unserialisable
config) + unstamped marker is a render error + lone `}}` refuses to compile +
static cache +
edit-without-restart + boot refusals (bad dir, missing pinned file) + pin
match/mismatch/drift + path traversal and symlink escape + a consumer-shaped
router serving the shell and a static file under the CSP layer. No network.
