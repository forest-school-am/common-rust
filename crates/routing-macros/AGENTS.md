# AGENTS.md — common-routing-macros

See `../routing/AGENTS.md` for the whole mechanism; this crate is its
`#[client]` attribute.

- `src/lib.rs` — the proc-macro shell: parse, expand, or emit the handler
  unchanged plus ONE `compile_error!` (so an opaque return reports itself and
  not a cascade of unresolved names).
- `src/parse.rs` — `parse_attr` (nothing or `link`), `describe` (extractor
  classification by the type's LAST path segment; guards and state skipped;
  binding name from the pattern), `response_of` (peels `Result`, tuples,
  `Json`/`ApiJson`; `StatusCode`/`()` → no content), `expand` (the export
  test). Unit tests at the bottom use `parse_quote!`.

Invariants: the macro reads ONE signature and no type definition; the
extractor and response idents are matched by name, so an app's wrapper must
be spelled `ApiPath`/`ApiQuery`/`ApiJson` (or the axum names) to be seen —
a wrapper under another name is silently a guard. The fqname it records is
`module_path!()::name`, which must equal `type_name` of the fn item the
router recorded; do not change one without the other.

`cargo test -p common-routing-macros` at the workspace root, in
`nix develop --impure`.
