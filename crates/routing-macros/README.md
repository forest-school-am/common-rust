# common-routing-macros

The `#[client]` attribute for [common-routing](../routing/README.md). Never
depended on directly: `common_routing::client` re-exports it, and the code it
expands to names `::common_routing::export::…`.

- `#[client]` on an `async fn` handler: reads `Path<T>` / `Query<T>` /
  `Json<T>` / `Multipart` (and `ApiPath`/`ApiQuery`/`ApiJson` wrappers) from
  the signature, and the response from the return type (`Json<T>` through
  `Result`, a `(StatusCode, Json<T>)` tuple or `ApiJson<T>`; `StatusCode` /
  `()` for no content). Everything else in the signature is skipped. An
  opaque return is a compile error naming the handler.
- `#[client(link)]`: the return is not inspected; the client gets a URL
  builder only.
- Emits, beside the untouched handler, `#[cfg(test)] #[test] fn
  export_client_<name>()` which, when `COMMON_ROUTING_EXPORT_DIR` is set,
  appends the descriptor to `$DIR/handlers.json` with the TypeScript names
  resolved through ts-rs at run time.

`src/parse.rs` holds the analysis as plain syn over an `ItemFn`, unit-tested
with `parse_quote!` (no trybuild): `cargo test -p common-routing-macros`.
