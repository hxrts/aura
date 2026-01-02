use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    parse::Parse, parse::ParseStream, parse_macro_input, Attribute, Data, DeriveInput, Fields,
    Ident, LitInt, LitStr, Result, Token,
};

struct DomainFactArgs {
    type_id: Option<LitStr>,
    schema_version: Option<LitInt>,
    context_field: Option<LitStr>,
    context_fn: Option<LitStr>,
}

impl DomainFactArgs {
    fn from_attrs(attrs: &[Attribute]) -> Result<Self> {
        let mut args = DomainFactArgs {
            type_id: None,
            schema_version: None,
            context_field: None,
            context_fn: None,
        };

        for attr in attrs {
            if !attr.path().is_ident("domain_fact") {
                continue;
            }
            let meta = attr.parse_args_with(DomainFactMeta::parse)?;
            for item in meta.items {
                match item.key.to_string().as_str() {
                    "type_id" => args.type_id = Some(item.value.clone()),
                    "schema_version" => {
                        if !item.is_int {
                            return Err(syn::Error::new(
                                item.key.span(),
                                "schema_version must be an integer literal",
                            ));
                        }
                        args.schema_version = item.int_value.clone();
                    }
                    "context" => args.context_field = Some(item.value.clone()),
                    "context_fn" => args.context_fn = Some(item.value.clone()),
                    other => {
                        return Err(syn::Error::new(
                            item.key.span(),
                            format!("unknown domain_fact attribute key: {other}"),
                        ))
                    }
                }
            }
        }

        Ok(args)
    }
}

struct DomainFactMeta {
    items: Vec<DomainFactMetaItem>,
}

struct DomainFactMetaItem {
    key: Ident,
    value: LitStr,
    int_value: Option<LitInt>,
    is_int: bool,
}

impl Parse for DomainFactMeta {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut items = Vec::new();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let value: syn::Lit = input.parse()?;
            let (lit_str, lit_int, is_int) = match value {
                syn::Lit::Str(s) => (s, None, false),
                syn::Lit::Int(i) => {
                    let lit_str = LitStr::new(&i.to_string(), i.span());
                    (lit_str, Some(i), true)
                }
                _ => {
                    return Err(syn::Error::new(
                        value.span(),
                        "expected string literal or integer literal",
                    ))
                }
            };
            items.push(DomainFactMetaItem {
                key,
                value: lit_str,
                int_value: lit_int,
                is_int,
            });
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self { items })
    }
}

pub fn derive_domain_fact_impl(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let args = match DomainFactArgs::from_attrs(&input.attrs) {
        Ok(args) => args,
        Err(err) => return err.to_compile_error().into(),
    };

    let type_id = match &args.type_id {
        Some(value) => value.clone(),
        None => {
            return syn::Error::new(
                Span::call_site(),
                "missing domain_fact attribute `type_id`, e.g. #[domain_fact(type_id = \"chat\", schema_version = 1)]",
            )
            .to_compile_error()
            .into();
        }
    };
    let schema_version = match &args.schema_version {
        Some(value) => value.clone(),
        None => {
            return syn::Error::new(
                Span::call_site(),
                "missing domain_fact attribute `schema_version`, e.g. #[domain_fact(schema_version = 1)]",
            )
            .to_compile_error()
            .into();
        }
    };

    if args.context_field.is_some() && args.context_fn.is_some() {
        return syn::Error::new(
            Span::call_site(),
            "domain_fact attributes `context` and `context_fn` are mutually exclusive",
        )
        .to_compile_error()
        .into();
    }

    let context_impl = match context_expr(&input.data, &args) {
        Ok(tokens) => tokens,
        Err(err) => return err.to_compile_error().into(),
    };

    let expanded = quote! {
        impl aura_journal::DomainFact for #name {
            fn type_id(&self) -> &'static str {
                #type_id
            }

            fn context_id(&self) -> aura_core::identifiers::ContextId {
                #context_impl
            }

            fn to_bytes(&self) -> Vec<u8> {
                aura_journal::encode_domain_fact(#type_id, #schema_version, self)
                    .expect("DomainFact encoding failed")
            }

            fn from_bytes(bytes: &[u8]) -> Option<Self>
            where
                Self: Sized,
            {
                aura_journal::decode_domain_fact(#type_id, #schema_version, bytes)
            }
        }
    };

    expanded.into()
}

fn context_expr(data: &Data, args: &DomainFactArgs) -> Result<TokenStream> {
    if let Some(context_fn) = &args.context_fn {
        let ident = Ident::new(&context_fn.value(), context_fn.span());
        return Ok(quote! { self.#ident() });
    }

    let field_name = args
        .context_field
        .as_ref()
        .map(|value| value.value())
        .unwrap_or_else(|| "context_id".to_string());
    let field_ident = Ident::new(&field_name, Span::call_site());

    match data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields) => {
                let mut found = false;
                for field in &fields.named {
                    if field.ident.as_ref() == Some(&field_ident) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        format!("expected field `{}` for DomainFact context_id", field_ident),
                    ));
                }
                Ok(quote! { self.#field_ident })
            }
            _ => Err(syn::Error::new(
                Span::call_site(),
                "DomainFact derive only supports structs with named fields or enums",
            )),
        },
        Data::Enum(data_enum) => {
            let mut arms = Vec::new();
            for variant in &data_enum.variants {
                let ident = &variant.ident;
                match &variant.fields {
                    Fields::Named(fields) => {
                        let mut has_field = false;
                        for field in &fields.named {
                            if field.ident.as_ref() == Some(&field_ident) {
                                has_field = true;
                                break;
                            }
                        }
                        if !has_field {
                            return Err(syn::Error::new(
                                variant.ident.span(),
                                format!(
                                    "variant `{}` missing `{}` field for DomainFact context_id",
                                    ident, field_ident
                                ),
                            ));
                        }
                        arms.push(quote! { Self::#ident { #field_ident, .. } => *#field_ident });
                    }
                    _ => {
                        return Err(syn::Error::new(
                            variant.ident.span(),
                            "DomainFact derive requires named fields for enum variants",
                        ))
                    }
                }
            }
            Ok(quote! {
                match self {
                    #(#arms),*
                }
            })
        }
        _ => Err(syn::Error::new(
            Span::call_site(),
            "DomainFact derive only supports structs or enums",
        )),
    }
}
