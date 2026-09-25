//! `#[client]`: the attribute shell — parse the attr, expand, emit. The
//! signature analysis belongs in `parse`; the descriptor data types and the
//! ts-rs type-name resolution belong in `common-routing`, not here.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

mod parse;

#[proc_macro_attribute]
pub fn client(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr = proc_macro2::TokenStream::from(attr);
    let item = parse_macro_input!(item as ItemFn);
    match parse::expand(attr, &item) {
        Ok(tokens) => tokens.into(),
        // Emit the handler beside the error, so callers see only the macro's
        // diagnostic and not a cascade of "cannot find" from the missing item.
        Err(e) => {
            let err = e.into_compile_error();
            quote!(#item #err).into()
        }
    }
}
