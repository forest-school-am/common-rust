//! `#[derive(NameColumns)]`: turn a row struct's `#[name(user)]`/`#[name(group)]`
//! fields into its static `NameTarget` table, keyed by `#[names(table = "…")]`.
//! What a target MEANS and how a rename is applied belongs in common-names, not
//! here — this macro only turns a struct's shape into `&'static [NameTarget]`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Error, Fields, LitStr};

#[proc_macro_derive(NameColumns, attributes(names, name))]
pub fn derive_name_columns(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn expand(input: &DeriveInput) -> syn::Result<Tokens> {
    let table = table_attr(input)?;
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(Error::new_spanned(
                    &input.ident,
                    "#[derive(NameColumns)] needs a struct with named fields",
                ))
            }
        },
        _ => {
            return Err(Error::new_spanned(
                &input.ident,
                "#[derive(NameColumns)] needs a struct with named fields",
            ))
        }
    };

    let mut targets = Vec::new();
    for field in fields {
        let Some(kind) = field_kind(field)? else {
            continue;
        };
        let ident = field.ident.as_ref().expect("named field");
        let column = LitStr::new(&ident.to_string(), ident.span());
        let kind = match kind {
            Kind::User => quote!(::common_names::NameKind::User),
            Kind::Group => quote!(::common_names::NameKind::Group),
        };
        targets.push(quote! {
            ::common_names::NameTarget {
                table: #table,
                column: #column,
                kind: #kind,
            }
        });
    }

    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::common_names::NameColumns for #ident #ty_generics #where_clause {
            fn name_targets() -> &'static [::common_names::NameTarget] {
                &[#(#targets),*]
            }
        }
    })
}

fn table_attr(input: &DeriveInput) -> syn::Result<LitStr> {
    let mut table: Option<LitStr> = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("names") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("table") {
                table = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("unknown #[names] key on a struct: expected `table`"))
            }
        })?;
    }
    table.ok_or_else(|| {
        Error::new_spanned(
            &input.ident,
            "#[derive(NameColumns)] needs #[names(table = \"…\")] naming the SQL table",
        )
    })
}

enum Kind {
    User,
    Group,
}

fn field_kind(field: &syn::Field) -> syn::Result<Option<Kind>> {
    let mut kind: Option<Kind> = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("name") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("user") {
                kind = Some(Kind::User);
                Ok(())
            } else if meta.path.is_ident("group") {
                kind = Some(Kind::Group);
                Ok(())
            } else {
                Err(meta.error("unknown #[name] kind on a field: expected `user` or `group`"))
            }
        })?;
    }
    Ok(kind)
}
