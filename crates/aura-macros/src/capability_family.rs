use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Error, Fields, Ident, ItemEnum, LitStr, Result as SynResult, Token, Variant};

pub fn capability_family_impl(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    match capability_family_impl_inner(attr.into(), item.into()) {
        Ok(output) => output.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

struct CapabilityFamilyAttr {
    namespace: LitStr,
}

impl Parse for CapabilityFamilyAttr {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let key: Ident = input.parse()?;
        if key != "namespace" {
            return Err(Error::new(key.span(), "expected `namespace = \"...\"`"));
        }
        input.parse::<Token![=]>()?;
        let namespace = input.parse::<LitStr>()?;
        if !input.is_empty() {
            return Err(input.error("unexpected extra tokens in capability_family attribute"));
        }
        Ok(Self { namespace })
    }
}

fn capability_family_impl_inner(attr: TokenStream, item: TokenStream) -> SynResult<TokenStream> {
    let attr = syn::parse2::<CapabilityFamilyAttr>(attr)?;
    let namespace = attr.namespace.value();
    validate_namespace(&attr.namespace, &namespace)?;

    let mut item_enum = syn::parse2::<ItemEnum>(item)?;
    let enum_ident = item_enum.ident.clone();

    let mut seen_local_names = HashSet::new();
    let mut variant_idents = Vec::new();
    let mut canonical_literals = Vec::new();

    for variant in &mut item_enum.variants {
        validate_unit_variant(variant)?;
        let local_name = extract_local_name(variant)?;
        if !seen_local_names.insert(local_name.value()) {
            return Err(Error::new(
                variant.span(),
                format!("duplicate capability local name `{}`", local_name.value()),
            ));
        }

        let canonical = if namespace.is_empty() {
            local_name.value()
        } else {
            format!("{namespace}:{}", local_name.value())
        };
        aura_core::CapabilityName::parse(&canonical).map_err(
            |error: aura_core::CapabilityNameError| {
                Error::new(local_name.span(), error.to_string())
            },
        )?;

        variant_idents.push(variant.ident.clone());
        canonical_literals.push(LitStr::new(&canonical, local_name.span()));
        variant
            .attrs
            .retain(|attr| !attr.path().is_ident("capability"));
    }

    Ok(quote! {
        #item_enum

        impl #enum_ident {
            pub fn as_name(&self) -> ::aura_core::CapabilityName {
                match self {
                    #(Self::#variant_idents => ::aura_core::capability_name!(#canonical_literals),)*
                }
            }

            pub fn declared_names() -> &'static [Self] {
                const DECLARED: &[#enum_ident] = &[#(#enum_ident::#variant_idents),*];
                DECLARED
            }
        }
    })
}

fn validate_namespace(namespace_lit: &LitStr, namespace: &str) -> SynResult<()> {
    if namespace.is_empty() {
        return Ok(());
    }
    if namespace.contains(':') {
        return Err(Error::new(
            namespace_lit.span(),
            "capability family namespace must be a single root segment",
        ));
    }
    aura_core::CapabilityName::parse(namespace).map_err(
        |error: aura_core::CapabilityNameError| Error::new(namespace_lit.span(), error.to_string()),
    )?;
    Ok(())
}

fn validate_unit_variant(variant: &Variant) -> SynResult<()> {
    if matches!(variant.fields, Fields::Unit) {
        Ok(())
    } else {
        Err(Error::new(
            variant.span(),
            "capability_family supports only fieldless enum variants",
        ))
    }
}

fn extract_local_name(variant: &Variant) -> SynResult<LitStr> {
    let mut local_name = None;

    for attr in &variant.attrs {
        if !attr.path().is_ident("capability") {
            continue;
        }

        if local_name.is_some() {
            return Err(Error::new(
                attr.span(),
                "duplicate #[capability(\"...\")] attribute",
            ));
        }

        local_name = Some(attr.parse_args::<LitStr>()?);
    }

    local_name.ok_or_else(|| {
        Error::new(
            variant.span(),
            "missing #[capability(\"...\")] attribute on capability family variant",
        )
    })
}
