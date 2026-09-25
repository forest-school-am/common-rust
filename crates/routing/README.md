# common-routing (+ common-routing-macros)

Every HTTP path written once, in Rust; the browser client compiled from it.
R114 cron item 4 (the review file, "Is the wire protocol maintained manually").

- **Version:** `0.3.0` · **Toolchain:** Rust 1.98.0 · axum 0.8 · ts-rs 10.
- Members of the `common-rust` workspace: `crates/routing` (this crate) and
  `crates/routing-macros` (the proc macro, re-exported as
  `common_routing::client`). Consumers take both through ONE path dependency
  on `common-routing` (`git+patch` on push day, like the other shared crates).

## The three parts

**1. Router.** `common_routing::Router<S>` has axum's method calls —
`.get(path, handler)`, `.post`, `.put`, `.delete`, `.patch`, `.route`,
`.nest`, `.merge`, `.layer`, `.route_layer`, `.with_state` — builds an
`axum::Router<S>` underneath and RECORDS each registration:
`Registration { fqname, method, path, path_params }`, where `fqname` is
`std::any::type_name` of the handler (`cron::web::run` for a plain
`async fn`) and `path_params` are the `{name}` segments of the template, in
order. Nested routers get their prefix applied. `.manifest()` returns the
table; `.write_manifest(path)` writes it sorted and pretty as `routes.json`;
`.into_axum()` hands over the router to serve. Nothing about types at the
bind.

`.get(path, handler)` is the RECORDING form. axum's own `.route(path,
get(handler))` is passed through unrecorded: the handler's name is gone once
it is inside a `MethodRouter`. Register through the method calls.

**2. `#[client]`.** On each `async fn` handler. From the signature: the
extractors that carry a client payload — `Path<T>` (a primitive → one
positional param; a tuple → by position; a struct → by field name),
`Query<T>`, `Json<T>`, `Multipart` (→ `FormData`), also as an app's
`ApiPath`/`ApiQuery`/`ApiJson` wrappers — and the response: the `T` of
`Json<T>`, through `Result<_, _>`, `(StatusCode, Json<T>)` or `ApiJson<T>`;
`StatusCode` / `()` for no content. State, guards, headers and anything else
are skipped. An opaque return (`impl IntoResponse`, `Response`, an alias
hiding one) is a compile error naming the handler. `#[client(link)]` is for
a handler the browser NAVIGATES to (a download): the return is not inspected
and the client gets `<name>Url(...)` only.

`#[client(path = "/api/groups/{group_name}")]` is for a handler whose path
params are read INSIDE a guard extractor — an auth check that does its own
`Path<…>` and hands the handler an already-authorized value. Such a handler
has no `Path<T>` in its signature, so the generator has nothing to bind the
route's params to and refuses. Declaring the template gives it the names
without a second, redundant extraction of the same segments. A bare `{param}`
is a `string`, which is what a URL segment is; `{id: number}` says otherwise
(`string`, `number`, `boolean`, `bigint`). Declaring a path on a handler that
already takes a `Path<T>` is an error — two sources for one thing can only
disagree — and a declared name that the mounted route does not have is caught
by the generator, naming both spellings. It composes with `link`.

The attribute emits, beside the untouched handler, `#[cfg(test)] #[test] fn
export_client_<name>()` — the ts-rs pattern: inert unless
`COMMON_ROUTING_EXPORT_DIR` is set, then it appends the descriptor to
`$DIR/handlers.json` (under a file lock; replace-by-fqname; sorted). The
TypeScript names are resolved in that test through ts-rs (`<T as
TS>::name()`; a struct path payload's fields from `inline()`), so ts-rs
stays for the DTOs and a name in `handlers.json` is a file in the app's
`bindings/`. The fqname is `module_path!()::name`, which equals the
router's `type_name` for a fn item.

**3. Generator.** `generate_client(routes_json, handlers_json, out_ts)` (or
`GenerateOptions::default().transport(..).types(..).include(..).generate(..)`)
joins the two tables by fqname and writes `client.ts`: one plain function per
handler, `snake_case` → `camelCase`, path params bound in TEMPLATE order,
then `query`, then `body`:

```ts
import { call } from "./client-call";
import type { RunDetail, RunPage, RunQuery, … } from "./index";
const enc = (v: string | number | boolean) => encodeURIComponent(String(v));

export function run(id: string, number: number): Promise<RunDetail> {
  return call<RunDetail>("GET", `/api/tasks/${enc(id)}/runs/${enc(number)}`);
}
export function runs(query: RunQuery): Promise<RunPage> {
  return call<RunPage>("GET", "/api/runs", query);
}
export function batchDownloadUrl(name: string, version: string): string {
  return `/api/batches/${enc(name)}/download/${enc(version)}`;
}
```

Errors name what is wrong: a `#[client]` handler no route mounted, an
included route whose handler has no descriptor, a handler mounted twice, a
template whose param count or names do not match the `Path<T>` payload, two
handlers that would become the same TypeScript name. `include` says which
routes must have a client function (an app's pages and static files do not;
its `/api/…` does).

**The transport.** `call(method, url, query?, body?)` is ~25 lines of
TypeScript: builds the query string (arrays as repeated keys; `undefined`,
`null` and `""` left out), sends JSON or a `FormData` as is, throws a
`CallFailure { status, body }` on a non-2xx, returns the JSON (or nothing on
204). It calls the global `fetch`, so a shim that wraps `window.fetch`
(common-oidc's 401 → re-auth) applies to every generated function. It is to
be served by common-ui beside the shim; until then each app vendors it.

## Recipe (what an app's justfile does)

```
COMMON_ROUTING_EXPORT_DIR=$staging cargo test -- export_routes export_client_
COMMON_ROUTING_EXPORT_DIR=$staging cargo test --test client -- generate_client
mv $staging/{routes.json,handlers.json,client.ts} crates/api/bindings/
```

`export_routes` is the app's own test (`routes().write_manifest(dir/
"routes.json")`); `generate_client` is the app's one-line test calling the
generator with its options. All three files are committed and diff-checked
like the ts-rs output. `tsc` then checks ordinary function signatures: a
renamed handler fails at the import, a changed param at the call.

## ts-rs and `#[serde(…)]` — read this before adding a serde attribute

During export ts-rs prints `failed to parse serde attribute … will be ignored`
for attributes it does not understand. It is noise most of the time and an
alarm some of the time, so the warnings stay ON: this crate does NOT enable
ts-rs's `no-serde-warnings` feature, and should not. ts-rs 11 behaves
identically here — bumping the pin does not remove the message.

What ts-rs understands on a field: `rename`, `skip`, `flatten`, `default`
(absorbed, but it does NOT make the field optional — it is accepted only to
avoid this warning) and `with`. On a container: `rename`, `rename_all`,
`rename_all_fields`, `tag`, `content`, `untagged`, `deny_unknown_fields`,
`bound`, `crate`. Anything else — `transparent`, `skip_serializing_if`,
`serialize_with`, `alias`, `borrow`, `other` — it does not.

THE TRAP, and the reason this section exists: an unknown key does not merely
get skipped. ts-rs parses the attribute as a whole and the first unknown key
aborts it, so the WHOLE `#[serde(…)]` is discarded — including keys it does
support that happen to sit in the same attribute. So this is safe:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]   // only `default`
                                                             // is lost, and it
                                                             // did nothing
```

and this silently generates the WRONG FIELD NAME, because the rename goes down
with the attribute that carried it:

```rust
#[serde(rename = "displayName", skip_serializing_if = "Option::is_none")]
```

The fix is to split them, since each `#[serde(…)]` is parsed on its own:

```rust
#[serde(rename = "displayName")]
#[serde(skip_serializing_if = "Option::is_none")]   // warns; nothing is lost
```

The same applies to `#[serde(transparent)]` on a container: a newtype's
TypeScript happens to come out right anyway, because ts-rs inlines a newtype
regardless — but any `rename_all` sharing that attribute is gone. When the
shape still disagrees, state it outright with `#[ts(as = "…")]` or
`#[ts(type = "…")]` rather than hoping the serde attribute carried.

## Tests

`nix develop --impure -c cargo test -p common-routing -p common-routing-macros`
at the workspace root. Router tests drive the axum service in-process
(tower oneshot); macro tests are unit tests over syn (no trybuild); the
pipeline test annotates handlers in a test file, exports both tables to a
temp dir and asserts the generated TypeScript.
