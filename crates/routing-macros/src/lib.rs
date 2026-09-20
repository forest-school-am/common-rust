//! `#[client]`: ONE handler's signature becomes a client descriptor and, beside
//! the untouched handler, a `#[test]` that writes it to `handlers.json` (the
//! ts-rs export pattern). Type names resolve in that test through ts-rs, so this
//! macro never needs to know what a type looks like — only WHERE in the request
//! it travels. The signature analysis lives in `parse`; this file is the shell.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

mod parse;

/// `#[client]` / `#[client(link)]` on an `async fn` axum handler.
///
/// Plain `#[client]` describes a JSON endpoint: the response is the `T` of
/// `Json<T>` (also through `Result<_, _>`, a `(StatusCode, Json<T>)` tuple,
/// or an `ApiJson<T>`-style wrapper); a bare `StatusCode` or `()` is a
/// no-content endpoint. Any other return type — `impl IntoResponse`, a bare
/// `Response`, a type alias hiding one — is a compile error naming the
/// handler: the client cannot be typed from something opaque.
///
/// `#[client(link)]` is for a handler the browser NAVIGATES to (a file
/// download): the return type is not inspected and the generated client
/// gets only a URL builder, `<name>Url(...)`.
#[proc_macro_attribute]
pub fn client(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr = proc_macro2::TokenStream::from(attr);
    let item = parse_macro_input!(item as ItemFn);
    match parse::expand(attr, &item) {
        Ok(tokens) => tokens.into(),
        // The handler is emitted unchanged beside the error so the only
        // diagnostic is the macro's own, not a cascade of "cannot find".
        Err(e) => {
            let err = e.into_compile_error();
            quote!(#item #err).into()
        }
    }
}
