//! AuraHandler derive macro implementation

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, format_ident};
use syn::{parse_macro_input, DeriveInput, Data, Fields, Meta, Attribute, Lit};

pub fn derive_aura_handler_impl(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    match generate_handler_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn generate_handler_impl(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    
    // Parse attributes for handler configuration
    let handler_config = parse_handler_attributes(&input.attrs)?;
    let use_async_trait = handler_config.use_async_trait;
    let error_type = handler_config.error_type;
    
    // Parse fields for state and config identification
    let fields = match &input.data {
        Data::Struct(data) => &data.fields,
        _ => return Err(syn::Error::new_spanned(input, "AuraHandler can only be derived for structs")),
    };
    
    let field_info = parse_handler_fields(fields)?;
    
    // Generate the handler trait name (convention: {Name}Handler -> {Name}HandlerTrait)
    let handler_trait_name = format_ident!("{}Trait", name);
    
    // Generate core handler methods
    let handler_methods = generate_handler_methods(&field_info, &error_type)?;
    
    // Generate the trait definition
    let trait_def = if use_async_trait {
        quote! {
            #[async_trait::async_trait]
            pub trait #handler_trait_name {
                type Error: std::error::Error + Send + Sync + 'static;
                
                #handler_methods
            }
        }
    } else {
        quote! {
            pub trait #handler_trait_name {
                type Error: std::error::Error + Send + Sync + 'static;
                
                #handler_methods
            }
        }
    };
    
    // Generate the implementation
    let trait_impl = if use_async_trait {
        quote! {
            #[async_trait::async_trait]
            impl #impl_generics #handler_trait_name for #name #ty_generics #where_clause {
                type Error = #error_type;
                
                // Implementation methods will be generated based on field types
                async fn handle_operation(&mut self, operation: Operation) -> Result<OperationResult, Self::Error> {
                    // Default implementation - can be overridden
                    todo!("Implement operation handling logic")
                }
                
                async fn get_state(&self) -> Result<serde_json::Value, Self::Error> {
                    // Default implementation accessing state field
                    Ok(serde_json::json!({}))
                }
            }
        }
    } else {
        quote! {
            impl #impl_generics #handler_trait_name for #name #ty_generics #where_clause {
                type Error = #error_type;
                
                fn handle_operation(&mut self, operation: Operation) -> Result<OperationResult, Self::Error> {
                    // Default implementation - can be overridden
                    todo!("Implement operation handling logic")
                }
                
                fn get_state(&self) -> Result<serde_json::Value, Self::Error> {
                    // Default implementation accessing state field
                    Ok(serde_json::json!({}))
                }
            }
        }
    };
    
    Ok(quote! {
        #trait_def
        #trait_impl
    })
}

#[derive(Default)]
struct HandlerConfig {
    use_async_trait: bool,
    error_type: TokenStream2,
}

fn parse_handler_attributes(attrs: &[Attribute]) -> syn::Result<HandlerConfig> {
    let mut config = HandlerConfig {
        error_type: quote! { aura_types::AuraError },
        ..Default::default()
    };
    
    for attr in attrs {
        if attr.path().is_ident("handler") {
            match &attr.meta {
                Meta::List(meta_list) => {
                    for token in meta_list.tokens.clone().into_iter() {
                        if let Ok(ident) = syn::parse2::<syn::Ident>(quote! { #token }) {
                            if ident == "async_trait" {
                                config.use_async_trait = true;
                            }
                        }
                    }
                }
                Meta::NameValue(meta_name_value) => {
                    if meta_name_value.path.is_ident("error") {
                        if let syn::Expr::Lit(expr_lit) = &meta_name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                let error_type_str = lit_str.value();
                                config.error_type = error_type_str.parse().unwrap_or_else(|_| {
                                    quote! { aura_types::AuraError }
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    
    Ok(config)
}

struct HandlerField {
    name: syn::Ident,
    field_type: HandlerFieldType,
}

#[derive(Debug)]
enum HandlerFieldType {
    State,
    Config,
    Regular,
}

fn parse_handler_fields(fields: &Fields) -> syn::Result<Vec<HandlerField>> {
    let mut handler_fields = Vec::new();
    
    match fields {
        Fields::Named(fields_named) => {
            for field in &fields_named.named {
                let field_name = field.ident.as_ref().unwrap().clone();
                let field_type = parse_field_attributes(&field.attrs)?;
                
                handler_fields.push(HandlerField {
                    name: field_name,
                    field_type,
                });
            }
        }
        _ => return Err(syn::Error::new_spanned(fields, "Only named fields are supported")),
    }
    
    Ok(handler_fields)
}

fn parse_field_attributes(attrs: &[Attribute]) -> syn::Result<HandlerFieldType> {
    for attr in attrs {
        if attr.path().is_ident("handler") {
            if let Meta::List(meta_list) = &attr.meta {
                for token in meta_list.tokens.clone().into_iter() {
                    if let Ok(ident) = syn::parse2::<syn::Ident>(quote! { #token }) {
                        match ident.to_string().as_str() {
                            "state" => return Ok(HandlerFieldType::State),
                            "config" => return Ok(HandlerFieldType::Config),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    
    Ok(HandlerFieldType::Regular)
}

fn generate_handler_methods(_field_info: &[HandlerField], _error_type: &TokenStream2) -> syn::Result<TokenStream2> {
    // For now, generate basic handler methods
    // This can be expanded based on field types and configuration
    Ok(quote! {
        async fn handle_operation(&mut self, operation: Operation) -> Result<OperationResult, Self::Error>;
        async fn get_state(&self) -> Result<serde_json::Value, Self::Error>;
    })
}