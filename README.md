# common-rust

The shared Rust crates behind the Les stand: configuration, logging, identity,
routing, and the server half of the common UI. One cargo workspace, twelve
crates, versioned together.

- **Version:** `0.3.0` (workspace-wide) · **Edition:** 2021 · **Toolchain:** Rust 1.98.0 · **License:** MIT

These crates are built for one deployment rather than for general use. They
assume [authentik](https://goauthentik.io/) as the identity provider, a
`DEPLOYMENT_TYPE` of `prod` or `dev` in the environment, and the conventions
described in each crate's README. Nothing here is published to crates.io.

## The crates

| Crate | Path | What it does |
| --- | --- | --- |
| `common-config` | `crates/config` | The config load pipeline: schema → args → merge → typed parse. Sources are defaults < file < env < args, later wins per field, every value stamped with its origin. |
| `common-config-derive` | `crates/config-derive` | `#[derive(Config)]`. One struct's fields become its schema and parser; a root declares `#[config(app = "…")]` and gains `Root`. |
| `common-logging` | `crates/logging` | Subscriber setup and the boot sequence. `boot` loads the binary's config tree, refuses any fault as a `startup` line, and brings up tracing from it. |
| `common-oidc` | `crates/oidc` | An OIDC client: PKCE browser flow, bearer validation, and group predicates for authorization. |
| `common-secrets` | `crates/secrets` | Fetching and caching application credentials. |
| `common-names` | `crates/names` | Keeps stored authentik usernames and group names fresh against a rename feed. |
| `common-names-derive` | `crates/names-derive` | `#[derive(NameColumns)]`. Turns a row struct's shape into the static table `common-names` applies renames through. |
| `common-routing` | `crates/routing` | Write each HTTP path once in Rust and compile the browser client from it. |
| `common-routing-macros` | `crates/routing-macros` | `#[client]`. The handler-signature analysis behind `common-routing`. |
| `common-templating` | `crates/templating` | Static file serving from a validated directory, with caching and path safety, plus shell stamping. |
| `common-theme` | `crates/theme` | Theme palettes and the browser theme loader, carried as committed consts. The loader owns the `les_theme` cookie contract. |
| `common-ui-core` | `crates/ui-core` | The common-ui assets as committed consts — sheets, component module, shell template, SRIs. No dependencies unless the `build` feature is on. |

Several crates carry a README of their own with the detail that matters when
you adopt them: `config`, `logging`, `oidc`, `routing`, `routing-macros`,
`templating`.

## Depend on it

The crates are workspace members, so a dependency points at the repository and
cargo selects the member by package name:

```toml
[dependencies]
common-logging = { git = "https://github.com/forest-school-am/common-rust.git", tag = "v0.3.0" }
common-config  = { git = "https://github.com/forest-school-am/common-rust.git", tag = "v0.3.0" }
```

## Build and test

The workspace pins its toolchain in `rust-toolchain.toml` and provides a nix
flake. The dev shell composes from a shared shell outside the repo, which makes
it impure:

```sh
nix develop --impure
cargo test --workspace
```

`cargo build` and `nix build` do not use the same compiler by default: the dev
shell's toolchain comes from fenix, while `nix build` uses nixpkgs'
`buildRustPackage`. That only matters if you are chasing a difference between
the two.

## Inside the Les stand

Stand builds do not go to the network. A single shared cargo patch at
`Les/.cargo/config.toml` redirects each of these dependencies to the local
working copy, so every repo under `Les/` builds against whatever `common-rust`
currently is. Cargo walks up from the build directory and *merges* every config
it finds, so that file applies to every sibling — a nearer
`.cargo/config.toml` does not shadow it, and no repo may keep one of its own.

Two consequences, both known:

- A repo that does not consume every patched crate carries an unused-patch
  record per crate it lacks, and those are emitted in non-deterministic order.
  `cargo --locked` is therefore unusable fleet-wide. It is a lock-check
  failure, not a build failure; discard the spurious `Cargo.lock` diff.
- A missing or wrong path in that file does not fail loudly — cargo falls back
  to the published crate and rewrites your lockfile to say so. Run
  `sh stand/check-cargo-patch.sh` if a build behaves oddly.

**Migration in progress.** That patch still keys on historical per-crate URLs
(`common-rust-logging.git`, `common-rust-oidc.git`,
`common-rust-templating.git`), none of which was ever a real remote — they
predate the merge of these crates into one workspace. Consumer manifests and
the patch move to the repository URL above together; until that happens, a
stand manifest keeps the per-crate spelling.
