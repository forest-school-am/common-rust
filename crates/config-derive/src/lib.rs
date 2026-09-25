//! `#[derive(Config)]`: ONE struct's fields become its `schema`/`from_values`
//! impl. How values are merged, spelled or rendered belongs in common-config —
//! this macro never sees another struct's fields and must stay that way: a
//! `nested` field is a CALL into the inner type's impl, not an expansion. A root
//! (`#[config(app = …)]`) also gets `Root`, reading only its own `deployment` and `log` fields.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{
    parse_macro_input, parse_quote, Data, DeriveInput, Error, Expr, ExprLit, Fields,
    GenericArgument, Lit, LitStr, Meta, PathArguments, Type,
};

#[proc_macro_derive(Config, attributes(config))]
pub fn derive_config(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

struct RootAttrs {
    app: Option<LitStr>,
    bin: Option<LitStr>,
}

#[derive(Default)]
struct FieldAttrs {
    default: Option<LitStr>,
    required: bool,
    secret: bool,
    nested: bool,
    prod_required: bool,
    dev_only: bool,
    env: Option<LitStr>,
    flag: Option<LitStr>,
    accepted: Option<LitStr>,
}

enum Shape {
    Bool,
    OptBool,
    Opt(Type),
    Plain(Type),
}

fn expand(input: &DeriveInput) -> syn::Result<Tokens> {
    let root = root_attrs(input)?;
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(Error::new_spanned(
                    &input.ident,
                    "#[derive(Config)] needs a struct with named fields",
                ))
            }
        },
        _ => {
            return Err(Error::new_spanned(
                &input.ident,
                "#[derive(Config)] needs a struct with named fields",
            ))
        }
    };

    let mut schema = Vec::new();
    let mut build = Vec::new();
    let mut log_type: Option<Type> = None;
    let mut has_deployment = false;
    for field in fields {
        let ident = field.ident.as_ref().expect("named field");
        let name = LitStr::new(&ident.to_string(), ident.span());
        let mut attrs = field_attrs(field)?;
        if attrs.nested {
            if root.app.is_some() && ident == "log" {
                log_type = Some(field.ty.clone());
            }
            if attrs.default.is_some()
                || attrs.required
                || attrs.secret
                || attrs.prod_required
                || attrs.dev_only
                || attrs.env.is_some()
                || attrs.flag.is_some()
                || attrs.accepted.is_some()
            {
                return Err(Error::new_spanned(
                    field,
                    "#[config(nested)] takes no other #[config] key: the inner struct declares its own",
                ));
            }
            let ty = &field.ty;
            schema.push(quote! {
                out.extend(<#ty as ::common_config::Config>::schema(&prefix.child(#name)));
            });
            build.push(quote! {
                #ident: <#ty as ::common_config::Config>::from_values(values, &prefix.child(#name))?,
            });
            continue;
        }

        let is_deployment = root.app.is_some() && ident == "deployment";
        if is_deployment {
            has_deployment = true;
            if attrs.default.is_some()
                || attrs.env.is_some()
                || attrs.flag.is_some()
                || attrs.accepted.is_some()
            {
                return Err(Error::new_spanned(
                    field,
                    "a root's `deployment` field is spelled by common-config (DEPLOYMENT_TYPE, \
                     required): it takes no default, env, flag or accepted",
                ));
            }
            attrs.required = true;
            attrs.env = Some(LitStr::new("DEPLOYMENT_TYPE", ident.span()));
        }
        let shape = shape(&field.ty);
        let presence = presence(field, &shape, &attrs)?;
        let help = match doc(&field.attrs) {
            text if text.is_empty() && is_deployment => quote!(::common_config::DEPLOYMENT_HELP),
            text => quote!(#text),
        };
        let class = class(field, &shape, &attrs)?;
        let secret = attrs.secret;
        let kind = match shape {
            Shape::Bool | Shape::OptBool => quote!(::common_config::Kind::Bool),
            _ => quote!(::common_config::Kind::Text),
        };
        let env = option_lit(&attrs.env);
        let flag = option_lit(&attrs.flag);
        let accepted = if is_deployment {
            quote!(::core::option::Option::Some(
                ::common_config::DEPLOYMENT_ACCEPTED
            ))
        } else {
            option_lit(&attrs.accepted)
        };
        schema.push(quote! {
            out.push(::common_config::Field {
                path: prefix.child(#name),
                help: #help,
                presence: #presence,
                class: #class,
                secret: #secret,
                kind: #kind,
                env: #env,
                flag: #flag,
            });
        });
        let read = match &shape {
            Shape::Bool => quote!(values.bool(&prefix.child(#name))?),
            Shape::OptBool => quote!(values.bool_opt(&prefix.child(#name))?),
            Shape::Plain(ty) => quote!(values.leaf::<#ty>(&prefix.child(#name), #accepted)?),
            Shape::Opt(ty) => quote!(values.leaf_opt::<#ty>(&prefix.child(#name), #accepted)?),
        };
        build.push(quote!(#ident: #read,));
    }

    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    if root.app.is_some() && !has_deployment {
        return Err(Error::new_spanned(
            &input.ident,
            "#[config(app = …)] needs a `deployment: common_config::Deployment` field",
        ));
    }
    let Some(log_type) = log_type.or_else(|| root.app.is_none().then(|| parse_quote!(()))) else {
        return Err(Error::new_spanned(
            &input.ident,
            "#[config(app = …)] needs a `#[config(nested)] log: …` field: the section \
             common_logging::boot initialises logging from",
        ));
    };
    let root_impl = root.app.as_ref().map(|app| {
        let bin = root
            .bin
            .clone()
            .unwrap_or_else(|| LitStr::new(&app.value().to_lowercase(), app.span()));
        quote! {
            impl #impl_generics ::common_config::Root for #ident #ty_generics #where_clause {
                const APP: &'static str = #app;
                const BIN: &'static str = #bin;
                type Log = #log_type;
                fn deployment(&self) -> ::common_config::Deployment {
                    self.deployment
                }
                fn log(&self) -> &#log_type {
                    &self.log
                }
            }
        }
    });

    Ok(quote! {
        impl #impl_generics ::common_config::Config for #ident #ty_generics #where_clause {
            fn schema(prefix: &::common_config::Path) -> ::std::vec::Vec<::common_config::Field> {
                let mut out = ::std::vec::Vec::new();
                #(#schema)*
                out
            }

            fn from_values(
                values: &::common_config::Values,
                prefix: &::common_config::Path,
            ) -> ::core::result::Result<Self, ::common_config::Refusal> {
                ::core::result::Result::Ok(Self { #(#build)* })
            }
        }
        #root_impl
    })
}

fn root_attrs(input: &DeriveInput) -> syn::Result<RootAttrs> {
    let mut out = RootAttrs {
        app: None,
        bin: None,
    };
    for attr in &input.attrs {
        if !attr.path().is_ident("config") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("app") {
                out.app = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("bin") {
                out.bin = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("unknown #[config] key on a struct: expected `app` or `bin`"))
            }
        })?;
    }
    if let (Some(bin), None) = (&out.bin, &out.app) {
        return Err(Error::new_spanned(
            bin,
            "#[config(bin = …)] needs #[config(app = …)] on the same struct",
        ));
    }
    if let Some(app) = &out.app {
        let value = app.value();
        let ok = !value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
        if !ok {
            return Err(Error::new_spanned(
                app,
                "#[config(app = …)] is the env prefix: ASCII upper-case letters and digits only, such as \"CRON\"",
            ));
        }
    }
    Ok(out)
}

fn field_attrs(field: &syn::Field) -> syn::Result<FieldAttrs> {
    let mut out = FieldAttrs::default();
    for attr in &field.attrs {
        if !attr.path().is_ident("config") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("default") {
                out.default = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("required") {
                out.required = true;
            } else if meta.path.is_ident("secret") {
                out.secret = true;
            } else if meta.path.is_ident("nested") {
                out.nested = true;
            } else if meta.path.is_ident("prod_required") {
                out.prod_required = true;
            } else if meta.path.is_ident("dev_only") {
                out.dev_only = true;
            } else if meta.path.is_ident("env") {
                out.env = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("flag") {
                out.flag = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("accepted") {
                out.accepted = Some(meta.value()?.parse()?);
            } else {
                return Err(meta.error(
                    "unknown #[config] key on a field: expected default, required, secret, nested, prod_required, dev_only, env, flag or accepted",
                ));
            }
            Ok(())
        })?;
    }
    if let Some(flag) = &out.flag {
        let value = flag.value();
        if value.starts_with('-') || value.is_empty() {
            return Err(Error::new_spanned(
                flag,
                "#[config(flag = …)] is the bare name without the leading dashes, such as \"data-dir\"",
            ));
        }
    }
    Ok(out)
}

fn class(field: &syn::Field, shape: &Shape, attrs: &FieldAttrs) -> syn::Result<Tokens> {
    match (attrs.prod_required, attrs.dev_only) {
        (false, false) => Ok(quote!(::common_config::Class::Neutral)),
        (true, true) => Err(Error::new_spanned(
            field,
            "a field is #[config(prod_required)] or #[config(dev_only)], not both",
        )),
        (true, false) => match shape {
            Shape::Opt(_) | Shape::OptBool if attrs.default.is_none() && !attrs.required => {
                Ok(quote!(::common_config::Class::ProdRequired))
            }
            _ => Err(Error::new_spanned(
                field,
                "#[config(prod_required)] needs an Option<T> field with no default: \
                 unset is legal under dev and refused under prod",
            )),
        },
        (false, true) => match shape {
            Shape::Opt(_) | Shape::OptBool | Shape::Bool
                if attrs.default.as_ref().is_none_or(|d| d.value() == "false")
                    && !attrs.required =>
            {
                Ok(quote!(::common_config::Class::DevOnly))
            }
            _ => Err(Error::new_spanned(
                field,
                "#[config(dev_only)] needs an Option<T> or bool field with no default: \
                 set under prod is refused",
            )),
        },
    }
}

fn presence(field: &syn::Field, shape: &Shape, attrs: &FieldAttrs) -> syn::Result<Tokens> {
    if attrs.default.is_some() && attrs.required {
        return Err(Error::new_spanned(
            field,
            "a field is either #[config(default = …)] or #[config(required)], not both",
        ));
    }
    match shape {
        Shape::Opt(_) | Shape::OptBool => {
            if attrs.default.is_some() || attrs.required {
                return Err(Error::new_spanned(
                    field,
                    "an Option<T> field is optional by its type: drop the Option, or drop default/required",
                ));
            }
            Ok(quote!(::common_config::Presence::Optional))
        }
        Shape::Bool => Ok(match (&attrs.default, attrs.required) {
            (Some(d), _) => quote!(::common_config::Presence::Default(#d)),
            (None, true) => quote!(::common_config::Presence::Required),
            (None, false) => quote!(::common_config::Presence::Default("false")),
        }),
        Shape::Plain(_) => match (&attrs.default, attrs.required) {
            (Some(d), _) => Ok(quote!(::common_config::Presence::Default(#d))),
            (None, true) => Ok(quote!(::common_config::Presence::Required)),
            (None, false) => Err(Error::new_spanned(
                field,
                "a field needs #[config(default = \"…\")], #[config(required)], or an Option<T> type",
            )),
        },
    }
}

fn shape(ty: &Type) -> Shape {
    if is_ident(ty, "bool") {
        return Shape::Bool;
    }
    match option_inner(ty) {
        Some(inner) if is_ident(inner, "bool") => Shape::OptBool,
        Some(inner) => Shape::Opt(inner.clone()),
        None => Shape::Plain(ty.clone()),
    }
}

fn is_ident(ty: &Type, name: &str) -> bool {
    matches!(ty, Type::Path(p) if p.qself.is_none() && p.path.is_ident(name))
}

fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(p) = ty else { return None };
    if p.qself.is_some() {
        return None;
    }
    let last = p.path.segments.last()?;
    if last.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    match args.args.first() {
        Some(GenericArgument::Type(inner)) if args.args.len() == 1 => Some(inner),
        _ => None,
    }
}

fn doc(attrs: &[syn::Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let Meta::NameValue(nv) = &attr.meta else {
            continue;
        };
        let Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) = &nv.value
        else {
            continue;
        };
        let line = s.value();
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            lines.push(trimmed.to_string());
        }
    }
    lines.join(" ")
}

fn option_lit(lit: &Option<LitStr>) -> Tokens {
    match lit {
        Some(s) => quote!(::core::option::Option::Some(#s)),
        None => quote!(::core::option::Option::None),
    }
}
