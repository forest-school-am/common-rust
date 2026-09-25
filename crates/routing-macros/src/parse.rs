//! Signature analysis behind `#[client]`: plain syn over an `ItemFn`. The
//! attribute shell and token emission belong in `lib.rs`.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Error, FnArg, GenericArgument, ItemFn, Meta, Pat, PathArguments, ReturnType, Token, Type,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Path,
    Query,
    Body,
    Multipart,
}

/// `ty` is `None` only for `Multipart`, which has no Rust payload type to name.
#[derive(Debug)]
pub struct Arg {
    pub name: String,
    pub kind: Kind,
    pub ty: Option<Type>,
}

#[derive(Debug)]
pub enum Response {
    Json(Type),
    NoContent,
    Link,
}

#[derive(Debug)]
pub struct Descriptor {
    pub name: syn::Ident,
    pub args: Vec<Arg>,
    pub response: Response,
}

const PATH: &[&str] = &["Path", "ApiPath"];
const QUERY: &[&str] = &["Query", "ApiQuery"];
const BODY: &[&str] = &["Json", "ApiJson"];
const MULTIPART: &[&str] = &["Multipart"];

/// What `#[client(…)]` carried. `path` is the DECLARED route template, for a
/// handler whose params are read inside a guard extractor and so never appear
/// as a `Path<T>` in the signature.
#[derive(Debug, Default)]
pub struct Attr {
    pub link: bool,
    pub path: Option<(String, Span)>,
}

/// TypeScript types a path segment may be given. `null` is deliberately absent:
/// a URL segment is always present, so it cannot be one.
const DECLARABLE: &[&str] = &["string", "number", "boolean", "bigint"];

pub fn parse_attr(attr: TokenStream) -> syn::Result<Attr> {
    let mut out = Attr::default();
    if attr.is_empty() {
        return Ok(out);
    }
    let span = attr.span();
    let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(attr)?;
    for meta in metas {
        match &meta {
            Meta::Path(p) if p.is_ident("link") => out.link = true,
            Meta::NameValue(nv) if nv.path.is_ident("path") => {
                let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = &nv.value
                else {
                    return Err(Error::new(
                        nv.value.span(),
                        "#[client(path = …)]: expected a string literal, the route template",
                    ));
                };
                out.path = Some((s.value(), s.span()));
            }
            other => {
                return Err(Error::new(
                    other.span(),
                    "#[client]: unknown option — accepted: `link`, `path = \"/a/{param}\"`",
                ))
            }
        }
    }
    if let Some((template, tspan)) = &out.path {
        // Parsed here so a bad template is a compile error on the attribute
        // rather than a generator error much later, in another process.
        let params = declared_params(template, *tspan)?;
        if params.is_empty() {
            return Err(Error::new(
                *tspan,
                format!(
                    "#[client(path = \"{template}\")]: the template declares no `{{param}}` — \
                     a handler with no path params does not need it"
                ),
            ));
        }
    }
    let _ = span;
    Ok(out)
}

/// The `{name}` / `{name: ts_type}` segments of a declared template, in order.
/// Defaults to `string`, which is what a URL segment is unless the handler
/// parses it into something else.
pub fn declared_params(template: &str, span: Span) -> syn::Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            return Err(Error::new(
                span,
                format!("#[client(path = \"{template}\")]: unclosed `{{` in the template"),
            ));
        };
        let body = &after[..close];
        rest = &after[close + 1..];

        let (name, ts) = match body.split_once(':') {
            Some((n, t)) => (n.trim(), t.trim()),
            None => (body.trim(), "string"),
        };
        if name.is_empty()
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            || name.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            return Err(Error::new(
                span,
                format!(
                    "#[client(path = \"{template}\")]: `{{{body}}}` is not a usable parameter name"
                ),
            ));
        }
        if !DECLARABLE.contains(&ts) {
            return Err(Error::new(
                span,
                format!(
                    "#[client(path = \"{template}\")]: `{{{body}}}` asks for TypeScript type \
                     `{ts}` — accepted: {}",
                    DECLARABLE.join(", ")
                ),
            ));
        }
        if out.iter().any(|(n, _)| n == name) {
            return Err(Error::new(
                span,
                format!("#[client(path = \"{template}\")]: `{{{name}}}` is declared twice"),
            ));
        }
        out.push((name.to_string(), ts.to_string()));
    }
    Ok(out)
}

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
        // axum's IntoResponse tuples carry the payload as the last element.
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

pub fn expand(attr: TokenStream, item: &ItemFn) -> syn::Result<TokenStream> {
    let attr = parse_attr(attr)?;
    let d = describe(item, attr.link)?;
    let name = &d.name;
    let test_name = format_ident!("export_client_{}", name, span = Span::call_site());

    // Two sources for one thing is worse than neither: if the signature already
    // carries the params, the declaration can only disagree with it.
    if let Some((template, tspan)) = &attr.path {
        if d.args.iter().any(|a| a.kind == Kind::Path) {
            return Err(Error::new(
                *tspan,
                format!(
                    "#[client(path = \"{template}\")] on `{name}`, which already takes a Path<T> \
                     — the declaration is for a handler whose params are read inside a guard, \
                     so drop whichever of the two is redundant"
                ),
            ));
        }
    }

    let mut args: Vec<TokenStream> = d
        .args
        .iter()
        .map(|a| {
            let arg_name = &a.name;
            let ty = &a.ty;
            match a.kind {
                Kind::Path => quote!(::common_routing::export::Arg::path::<#ty>(#arg_name)),
                Kind::Query => quote!(::common_routing::export::Arg::query::<#ty>(#arg_name)),
                Kind::Body => quote!(::common_routing::export::Arg::body::<#ty>(#arg_name)),
                Kind::Multipart => quote!(::common_routing::export::Arg::multipart(#arg_name)),
            }
        })
        .collect();

    // A declared template stands in for the Path<T> the handler does not take.
    // The descriptor it produces is the struct-payload shape, so the generator
    // binds it by field name exactly as it binds a real `Path<SomeStruct>`.
    if let Some((template, tspan)) = &attr.path {
        let pairs = declared_params(template, *tspan)?
            .into_iter()
            .map(|(n, t)| quote!((#n, #t)));
        args.push(quote!(
            ::common_routing::export::Arg::declared_path(#template, &[#(#pairs),*])
        ));
    }

    let response = match &d.response {
        Response::Json(ty) => quote!(::common_routing::export::Response::json::<#ty>()),
        Response::NoContent => quote!(::common_routing::export::Response::NoContent),
        Response::Link => quote!(::common_routing::export::Response::Link),
    };
    Ok(quote! {
        #item

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
        assert!(!parse_attr(quote!()).unwrap().link);
        assert!(parse_attr(quote!(link)).unwrap().link);
        assert!(parse_attr(quote!(links)).is_err());
        assert_eq!(
            parse_attr(quote!(path = "/a/{x}")).unwrap().path.unwrap().0,
            "/a/{x}"
        );
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

    // --- #[client(path = …)]: params declared, because a guard extractor ate them ---

    fn guarded() -> ItemFn {
        parse_quote! {
            async fn group(_g: GroupAccess) -> Json<Group> { todo!() }
        }
    }

    #[test]
    fn a_declared_template_stands_in_for_the_path_the_handler_never_takes() {
        let out = expand(quote!(path = "/api/groups/{group_name}"), &guarded())
            .expect("expands")
            .to_string();
        assert!(out.contains("declared_path"), "{out}");
        assert!(out.contains(r#""/api/groups/{group_name}""#), "{out}");
        assert!(out.contains(r#""group_name""#), "{out}");
        // A bare {param} is a string: that is what a URL segment is.
        assert!(out.contains(r#""string""#), "{out}");
    }

    #[test]
    fn a_declared_param_may_name_its_typescript_type() {
        let out = expand(
            quote!(path = "/api/groups/{name}/members/{id: number}"),
            &guarded(),
        )
        .expect("expands")
        .to_string();
        assert!(out.contains(r#""number""#), "{out}");
        assert!(out.contains(r#""id""#), "{out}");
    }

    #[test]
    fn link_and_a_declared_path_compose() {
        let item: ItemFn = parse_quote! {
            async fn download(_g: GroupAccess) -> axum::response::Response { todo!() }
        };
        let out = expand(
            quote!(link, path = "/api/groups/{group_name}/export"),
            &item,
        )
        .expect("expands")
        .to_string();
        assert!(out.contains("Response :: Link"), "{out}");
        assert!(out.contains("declared_path"), "{out}");
    }

    #[test]
    fn declaring_a_path_a_handler_already_takes_is_two_sources_for_one_thing() {
        let item: ItemFn = parse_quote! {
            async fn group(ApiPath(id): ApiPath<String>) -> Json<Group> { todo!() }
        };
        let err = expand(quote!(path = "/api/groups/{group_name}"), &item)
            .expect_err("refuses")
            .to_string();
        assert!(err.contains("already takes a Path<T>"), "{err}");
    }

    #[test]
    fn a_template_with_no_params_does_not_need_declaring() {
        let err = expand(quote!(path = "/api/groups"), &guarded())
            .expect_err("refuses")
            .to_string();
        assert!(err.contains("declares no"), "{err}");
    }

    #[test]
    fn a_declared_param_may_not_ask_for_an_arbitrary_typescript_type() {
        let err = expand(quote!(path = "/api/groups/{g: Group}"), &guarded())
            .expect_err("refuses")
            .to_string();
        assert!(
            err.contains("accepted: string, number, boolean, bigint"),
            "{err}"
        );
    }

    #[test]
    fn the_same_param_twice_is_a_mistake_not_a_merge() {
        let err = expand(quote!(path = "/a/{x}/b/{x}"), &guarded())
            .expect_err("refuses")
            .to_string();
        assert!(err.contains("declared twice"), "{err}");
    }

    #[test]
    fn an_unclosed_brace_is_caught_on_the_attribute_not_much_later() {
        let err = expand(quote!(path = "/a/{x"), &guarded())
            .expect_err("refuses")
            .to_string();
        assert!(err.contains("unclosed"), "{err}");
    }

    #[test]
    fn declared_params_reads_names_and_types_in_order() {
        let got = declared_params("/a/{one}/b/{two: number}/c", Span::call_site()).unwrap();
        assert_eq!(
            got,
            vec![
                ("one".to_string(), "string".to_string()),
                ("two".to_string(), "number".to_string()),
            ]
        );
    }
}
