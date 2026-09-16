//! The signature analysis behind `#[client]`, as plain syn over an `ItemFn`
//! so it can be unit-tested without a compiler in the loop.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Error, FnArg, GenericArgument, ItemFn, Pat, PathArguments, ReturnType, Type};

/// Where in the request an argument travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Path,
    Query,
    Body,
    Multipart,
}

/// One extractor the client has to supply. `ty` is the payload type (the `T`
/// of `Path<T>`); `None` only for `Multipart`, which has no Rust type to name.
#[derive(Debug)]
pub struct Arg {
    pub name: String,
    pub kind: Kind,
    pub ty: Option<Type>,
}

#[derive(Debug)]
pub enum Response {
    /// `Json<T>` in some wrapping: the client resolves to `T`.
    Json(Type),
    /// `StatusCode` / `()`: the client resolves to nothing (204-style).
    NoContent,
    /// `#[client(link)]`: not inspected; only a URL builder is generated.
    Link,
}

#[derive(Debug)]
pub struct Descriptor {
    pub name: syn::Ident,
    pub args: Vec<Arg>,
    pub response: Response,
}

/// Extractor idents recognised by their LAST path segment, so `axum::Json`,
/// `Json` and an app's `ApiJson` wrapper all count. The `Api*` names are the
/// wrappers Les apps use to answer a malformed request with the contract's
/// rejection rather than axum's plain text; they carry the same payload.
const PATH: &[&str] = &["Path", "ApiPath"];
const QUERY: &[&str] = &["Query", "ApiQuery"];
const BODY: &[&str] = &["Json", "ApiJson"];
const MULTIPART: &[&str] = &["Multipart"];

/// `#[client]` → `false`; `#[client(link)]` → `true`; anything else is an error.
pub fn parse_attr(attr: TokenStream) -> syn::Result<bool> {
    let text = attr.to_string();
    match text.trim() {
        "" => Ok(false),
        "link" => Ok(true),
        other => Err(Error::new(
            attr.span(),
            format!("#[client]: unknown option `{other}` — accepted: nothing, or `link`"),
        )),
    }
}

/// The last segment of a path type plus its first generic argument.
fn last_segment(ty: &Type) -> Option<(&syn::PathSegment, Option<&Type>)> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    let first = match &seg.arguments {
        PathArguments::AngleBracketed(a) => a.args.iter().find_map(|a| match a {
            GenericArgument::Type(t) => Some(t),
            _ => None,
        }),
        _ => None,
    };
    Some((seg, first))
}

fn classify(ty: &Type) -> syn::Result<Option<(Kind, Option<Type>)>> {
    let Some((seg, inner)) = last_segment(ty) else {
        return Ok(None);
    };
    let ident = seg.ident.to_string();
    let kind = if PATH.contains(&ident.as_str()) {
        Kind::Path
    } else if QUERY.contains(&ident.as_str()) {
        Kind::Query
    } else if BODY.contains(&ident.as_str()) {
        Kind::Body
    } else if MULTIPART.contains(&ident.as_str()) {
        return Ok(Some((Kind::Multipart, None)));
    } else {
        return Ok(None);
    };
    match inner {
        Some(t) => Ok(Some((kind, Some(t.clone())))),
        None => Err(Error::new(
            ty.span(),
            format!("#[client]: `{ident}` needs its payload type argument (`{ident}<T>`)"),
        )),
    }
}

/// The binding's name, for the descriptor: `ApiPath(id)` → `id`, `q` → `q`;
/// a tuple pattern or anything else falls back to the kind's word.
fn pat_name(pat: &Pat, kind: Kind) -> String {
    fn ident_of(pat: &Pat) -> Option<String> {
        match pat {
            Pat::Ident(p) => Some(p.ident.to_string()),
            Pat::TupleStruct(ts) if ts.elems.len() == 1 => ident_of(&ts.elems[0]),
            Pat::Paren(p) => ident_of(&p.pat),
            _ => None,
        }
    }
    ident_of(pat).unwrap_or_else(|| {
        match kind {
            Kind::Path => "path",
            Kind::Query => "query",
            Kind::Body => "body",
            Kind::Multipart => "multipart",
        }
        .to_owned()
    })
}

/// Peels `Result<X, _>` / `Result<X>`, then names `X`'s payload.
fn response_of(name: &str, ty: &Type) -> syn::Result<Response> {
    let opaque = || {
        Error::new(
            ty.span(),
            format!(
                "#[client] on `{name}`: the return type `{}` is opaque — return Json<T> \
                 (also as Result<Json<T>, _>, (StatusCode, Json<T>) or ApiJson<T>), or \
                 StatusCode / () for no content; a handler the browser navigates to takes \
                 #[client(link)]",
                quote!(#ty)
            ),
        )
    };
    match ty {
        Type::Tuple(t) if t.elems.is_empty() => Ok(Response::NoContent),
        // `(StatusCode, Json<T>)`, `(StatusCode, HeaderMap, Json<T>)`: the
        // payload is the LAST element, as axum's IntoResponse tuples require.
        Type::Tuple(t) => match t.elems.last() {
            Some(last) => match last_segment(last) {
                Some((seg, Some(inner))) if BODY.contains(&seg.ident.to_string().as_str()) => {
                    Ok(Response::Json(inner.clone()))
                }
                _ => Err(opaque()),
            },
            None => Err(opaque()),
        },
        Type::Path(_) => {
            let Some((seg, inner)) = last_segment(ty) else {
                return Err(opaque());
            };
            let ident = seg.ident.to_string();
            match (ident.as_str(), inner) {
                ("Result", Some(inner)) => response_of(name, inner),
                (_, Some(inner)) if BODY.contains(&ident.as_str()) => {
                    Ok(Response::Json(inner.clone()))
                }
                ("StatusCode", None) => Ok(Response::NoContent),
                _ => Err(opaque()),
            }
        }
        _ => Err(opaque()),
    }
}

/// The descriptor of one handler, or the error the macro reports.
pub fn describe(item: &ItemFn, link: bool) -> syn::Result<Descriptor> {
    let name = item.sig.ident.clone();
    if item.sig.asyncness.is_none() {
        return Err(Error::new(
            item.sig.span(),
            format!("#[client] on `{name}`: expected an `async fn` handler"),
        ));
    }
    let mut args = Vec::new();
    for input in &item.sig.inputs {
        let FnArg::Typed(pt) = input else {
            return Err(Error::new(
                input.span(),
                format!("#[client] on `{name}`: a handler has no `self`"),
            ));
        };
        if let Some((kind, ty)) = classify(&pt.ty)? {
            args.push(Arg {
                name: pat_name(&pt.pat, kind),
                kind,
                ty,
            });
        }
        // Anything else — State, guards, HeaderMap, custom extractors without
        // a client-visible payload — is the server's business.
    }
    let response = if link {
        Response::Link
    } else {
        match &item.sig.output {
            ReturnType::Default => Response::NoContent,
            ReturnType::Type(_, ty) => response_of(&name.to_string(), ty)?,
        }
    };
    Ok(Descriptor {
        name,
        args,
        response,
    })
}

/// The handler, unchanged, plus its export test.
pub fn expand(attr: TokenStream, item: &ItemFn) -> syn::Result<TokenStream> {
    let link = parse_attr(attr)?;
    let d = describe(item, link)?;
    let name = &d.name;
    let test_name = format_ident!("export_client_{}", name, span = Span::call_site());
    let args = d.args.iter().map(|a| {
        let arg_name = &a.name;
        let ty = &a.ty;
        match a.kind {
            Kind::Path => quote!(::common_routing::export::Arg::path::<#ty>(#arg_name)),
            Kind::Query => quote!(::common_routing::export::Arg::query::<#ty>(#arg_name)),
            Kind::Body => quote!(::common_routing::export::Arg::body::<#ty>(#arg_name)),
            Kind::Multipart => quote!(::common_routing::export::Arg::multipart(#arg_name)),
        }
    });
    let response = match &d.response {
        Response::Json(ty) => quote!(::common_routing::export::Response::json::<#ty>()),
        Response::NoContent => quote!(::common_routing::export::Response::NoContent),
        Response::Link => quote!(::common_routing::export::Response::Link),
    };
    Ok(quote! {
        #item

        // The descriptor's export, on the ts-rs pattern: a test that does
        // nothing unless COMMON_ROUTING_EXPORT_DIR is set, so `cargo test`
        // stays silent and the codegen recipe runs it with the variable set.
        #[cfg(test)]
        #[test]
        fn #test_name() {
            let Some(dir) = ::common_routing::export::dir() else { return; };
            let handler = ::common_routing::export::Handler {
                fqname: concat!(module_path!(), "::", stringify!(#name)).to_owned(),
                name: stringify!(#name).to_owned(),
                args: vec![#(#args),*],
                response: #response,
            };
            ::common_routing::export::append(&dir, handler)
                .expect("writing the handler descriptor to handlers.json");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn kinds(item: &ItemFn) -> Vec<(String, Kind, Option<String>)> {
        describe(item, false)
            .expect("describable")
            .args
            .into_iter()
            .map(|a| (a.name, a.kind, a.ty.map(|t| quote!(#t).to_string())))
            .collect()
    }

    #[test]
    fn extractors_are_read_in_order_and_guards_are_skipped() {
        let item: ItemFn = parse_quote! {
            async fn run(
                State(app): State<Arc<App>>,
                _who: Operator,
                ApiPath((task_id, number)): ApiPath<(String, i64)>,
                headers: HeaderMap,
                ApiQuery(q): ApiQuery<RunQuery>,
                Json(body): Json<CreateTaskRequest>,
                mut multipart: Multipart,
            ) -> Result<Json<RunDetail>, AppError> { todo!() }
        };
        assert_eq!(
            kinds(&item),
            vec![
                ("path".into(), Kind::Path, Some("(String , i64)".into())),
                ("q".into(), Kind::Query, Some("RunQuery".into())),
                ("body".into(), Kind::Body, Some("CreateTaskRequest".into())),
                ("multipart".into(), Kind::Multipart, None),
            ]
        );
    }

    #[test]
    fn a_single_binding_keeps_its_name() {
        let item: ItemFn = parse_quote! {
            async fn task(ApiPath(id): ApiPath<String>) -> Json<TaskDetail> { todo!() }
        };
        assert_eq!(
            kinds(&item),
            vec![("id".into(), Kind::Path, Some("String".into()))]
        );
    }

    fn response(ret: TokenStream) -> Response {
        let item: ItemFn = parse_quote! { async fn h() -> #ret { todo!() } };
        describe(&item, false).expect("describable").response
    }

    fn json_of(r: Response) -> String {
        match r {
            Response::Json(t) => quote!(#t).to_string(),
            other => panic!("expected Json, got {other:?}"),
        }
    }

    #[test]
    fn every_json_wrapping_resolves_to_its_payload() {
        assert_eq!(json_of(response(quote!(Json<SessionInfo>))), "SessionInfo");
        assert_eq!(
            json_of(response(quote!(ApiJson<SessionInfo>))),
            "SessionInfo"
        );
        assert_eq!(
            json_of(response(quote!(Result<Json<Vec<TaskSummary>>>))),
            "Vec < TaskSummary >"
        );
        assert_eq!(
            json_of(response(quote!(Result<Json<RunPage>, AppError>))),
            "RunPage"
        );
        assert_eq!(
            json_of(response(quote!(Result<(StatusCode, Json<TaskCreated>)>))),
            "TaskCreated"
        );
        assert_eq!(
            json_of(response(quote!(std::result::Result<axum::Json<X>, E>))),
            "X"
        );
    }

    #[test]
    fn status_code_and_unit_are_no_content() {
        assert!(matches!(
            response(quote!(Result<StatusCode>)),
            Response::NoContent
        ));
        assert!(matches!(response(quote!(StatusCode)), Response::NoContent));
        assert!(matches!(response(quote!(())), Response::NoContent));
        let item: ItemFn = parse_quote! { async fn h() { } };
        assert!(matches!(
            describe(&item, false).unwrap().response,
            Response::NoContent
        ));
    }

    #[test]
    fn opaque_returns_are_errors_naming_the_handler() {
        for ret in [
            quote!(impl IntoResponse),
            quote!(Response),
            quote!(Result),
            quote!(Result<Response, AppError>),
            quote!(Html<String>),
            quote!(&'static str),
        ] {
            let item: ItemFn = parse_quote! { async fn opaque_one() -> #ret { todo!() } };
            let err = describe(&item, false).expect_err("opaque").to_string();
            assert!(err.contains("`opaque_one`"), "{err}");
            assert!(err.contains("opaque"), "{err}");
        }
    }

    #[test]
    fn link_skips_the_return_type() {
        let item: ItemFn = parse_quote! {
            async fn download(ApiPath((n, v)): ApiPath<(String, String)>) -> Result { todo!() }
        };
        let d = describe(&item, true).expect("link");
        assert!(matches!(d.response, Response::Link));
        assert_eq!(d.args.len(), 1);
    }

    #[test]
    fn a_wrapper_without_its_payload_type_is_an_error() {
        let item: ItemFn = parse_quote! { async fn h(p: Path) -> Json<X> { todo!() } };
        let err = describe(&item, false).expect_err("missing T").to_string();
        assert!(err.contains("`Path<T>`"), "{err}");
    }

    #[test]
    fn a_non_async_fn_is_an_error() {
        let item: ItemFn = parse_quote! { fn h() -> Json<X> { todo!() } };
        assert!(describe(&item, false).is_err());
    }

    #[test]
    fn attr_grammar() {
        assert!(!parse_attr(quote!()).unwrap());
        assert!(parse_attr(quote!(link)).unwrap());
        assert!(parse_attr(quote!(links)).is_err());
    }

    #[test]
    fn expansion_keeps_the_handler_and_adds_the_export_test() {
        let item: ItemFn = parse_quote! {
            pub(crate) async fn task(ApiPath(id): ApiPath<String>) -> Result<Json<TaskDetail>> { todo!() }
        };
        let out = expand(quote!(), &item).expect("expands").to_string();
        assert!(out.contains("async fn task"));
        assert!(out.contains("fn export_client_task"));
        assert!(out.contains("Arg :: path :: < String >"));
        assert!(out.contains("Response :: json :: < TaskDetail >"));
        assert!(out.contains("COMMON_ROUTING") || out.contains("export :: dir"));
    }
}
