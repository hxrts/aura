//! Effect Handler Generation Macros
//!
//! This module provides procedural macros for generating effect handler implementations
//! with consistent patterns for mock/real variants, eliminating boilerplate code.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    token::Comma,
    Error, ExprBlock, Ident, Path, Result, Token, Type,
};

/// Configuration for an effect handler implementation
#[derive(Debug, Clone)]
pub struct EffectHandlerConfig {
    pub trait_name: Path,
    #[allow(dead_code)] // Reserved for future error handling features
    pub error_type: Option<Path>,
    pub mock_handler: HandlerVariant,
    pub real_handler: HandlerVariant,
}

/// Configuration for a handler variant (mock or real)
#[derive(Debug, Clone)]
pub struct HandlerVariant {
    pub struct_name: Ident,
    pub state_fields: Vec<StateField>,
    pub methods: Vec<MethodImpl>,
    pub imports: Vec<Path>,
    pub features: HandlerFeatures,
}

/// State field definition
#[derive(Debug, Clone)]
pub struct StateField {
    pub name: Ident,
    pub field_type: Type,
}

/// Method implementation
#[derive(Debug, Clone)]
pub struct MethodImpl {
    pub name: Ident,
    pub params: Vec<(Ident, Type)>,
    pub return_type: Type,
    pub body: ExprBlock,
}

/// Handler-specific features
#[derive(Debug, Clone, Default)]
pub struct HandlerFeatures {
    pub disallowed_methods: bool,
    pub disallowed_types: bool,
    pub async_trait: bool,
    pub deterministic: bool,
}

impl Parse for EffectHandlerConfig {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut trait_name = None;
        let mut error_type = None;
        let mut mock_handler = None;
        let mut real_handler = None;

        while !input.is_empty() {
            let field_name: Ident = input.parse()?;
            input.parse::<Token![:]>()?;

            match field_name.to_string().as_str() {
                "trait_name" => {
                    trait_name = Some(input.parse()?);
                }
                "error_type" => {
                    error_type = Some(input.parse()?);
                }
                "mock" => {
                    let content;
                    syn::braced!(content in input);
                    mock_handler = Some(HandlerVariant::parse(&content)?);
                }
                "real" => {
                    let content;
                    syn::braced!(content in input);
                    real_handler = Some(HandlerVariant::parse(&content)?);
                }
                _ => {
                    return Err(Error::new(
                        field_name.span(),
                        format!("Unknown field: {}", field_name),
                    ));
                }
            }

            // Parse optional comma
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        let trait_name = trait_name.ok_or_else(|| Error::new(Span::call_site(), "trait_name is required"))?;
        let mock_handler = mock_handler.ok_or_else(|| Error::new(Span::call_site(), "mock handler is required"))?;
        let real_handler = real_handler.ok_or_else(|| Error::new(Span::call_site(), "real handler is required"))?;

        Ok(EffectHandlerConfig {
            trait_name,
            error_type,
            mock_handler,
            real_handler,
        })
    }
}

impl HandlerVariant {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut struct_name = None;
        let mut state_fields = Vec::new();
        let mut methods = Vec::new();
        let mut imports = Vec::new();
        let mut features = HandlerFeatures::default();

        while !input.is_empty() {
            let field_name: Ident = input.parse()?;
            input.parse::<Token![:]>()?;

            match field_name.to_string().as_str() {
                "struct_name" => {
                    struct_name = Some(input.parse()?);
                }
                "state" => {
                    let content;
                    syn::braced!(content in input);
                    state_fields = Self::parse_state_fields(&content)?;
                }
                "methods" => {
                    let content;
                    syn::braced!(content in input);
                    methods = Self::parse_methods(&content)?;
                }
                "imports" => {
                    let content;
                    syn::bracketed!(content in input);
                    let paths: Punctuated<Path, Comma> = content.parse_terminated(Path::parse, Token![,])?;
                    imports = paths.into_iter().collect();
                }
                "features" => {
                    let content;
                    syn::braced!(content in input);
                    features = Self::parse_features(&content)?;
                }
                _ => {
                    return Err(Error::new(
                        field_name.span(),
                        format!("Unknown field: {}", field_name),
                    ));
                }
            }

            // Parse optional comma
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        let struct_name = struct_name.ok_or_else(|| Error::new(Span::call_site(), "struct_name is required"))?;

        Ok(HandlerVariant {
            struct_name,
            state_fields,
            methods,
            imports,
            features,
        })
    }

    fn parse_state_fields(input: ParseStream) -> Result<Vec<StateField>> {
        let mut fields = Vec::new();

        while !input.is_empty() {
            let name: Ident = input.parse()?;
            input.parse::<Token![:]>()?;
            let field_type: Type = input.parse()?;

            fields.push(StateField { name, field_type });

            // Parse optional comma
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(fields)
    }

    fn parse_methods(input: ParseStream) -> Result<Vec<MethodImpl>> {
        let mut methods = Vec::new();

        while !input.is_empty() {
            let name: Ident = input.parse()?;
            
            // Parse parameters in parentheses
            let content;
            syn::parenthesized!(content in input);
            let mut params = Vec::new();
            
            while !content.is_empty() {
                let param_name: Ident = content.parse()?;
                content.parse::<Token![:]>()?;
                let param_type: Type = content.parse()?;
                params.push((param_name, param_type));
                
                if content.peek(Token![,]) {
                    content.parse::<Token![,]>()?;
                }
            }

            // Parse return type
            input.parse::<Token![->]>()?;
            let return_type: Type = input.parse()?;

            // Parse fat arrow and body
            input.parse::<Token![=>]>()?;
            let body: ExprBlock = input.parse()?;

            methods.push(MethodImpl {
                name,
                params,
                return_type,
                body,
            });

            // Parse optional comma
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(methods)
    }

    fn parse_features(input: ParseStream) -> Result<HandlerFeatures> {
        let mut features = HandlerFeatures::default();

        while !input.is_empty() {
            let feature_name: Ident = input.parse()?;
            input.parse::<Token![:]>()?;
            let feature_value: syn::LitBool = input.parse()?;

            match feature_name.to_string().as_str() {
                "disallowed_methods" => features.disallowed_methods = feature_value.value,
                "disallowed_types" => features.disallowed_types = feature_value.value,
                "async_trait" => features.async_trait = feature_value.value,
                "deterministic" => features.deterministic = feature_value.value,
                _ => {
                    return Err(Error::new(
                        feature_name.span(),
                        format!("Unknown feature: {}", feature_name),
                    ));
                }
            }

            // Parse optional comma
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(features)
    }
}

/// Generate the complete effect handler implementation
pub fn generate_effect_handlers(config: EffectHandlerConfig) -> TokenStream2 {
    let mock_impl = generate_handler_impl(&config.trait_name, &config.mock_handler, true);
    let real_impl = generate_handler_impl(&config.trait_name, &config.real_handler, false);

    quote! {
        #mock_impl
        #real_impl
    }
}

/// Generate implementation for a single handler variant
fn generate_handler_impl(trait_name: &Path, handler: &HandlerVariant, is_mock: bool) -> TokenStream2 {
    let struct_name = &handler.struct_name;
    let state_fields = generate_state_fields(&handler.state_fields);
    let constructor = generate_constructor(struct_name, &handler.state_fields, is_mock);
    let default_impl = generate_default_impl(struct_name, is_mock);
    let trait_impl = generate_trait_impl(trait_name, struct_name, &handler.methods, &handler.features);
    let imports = generate_imports(&handler.imports);
    let feature_attrs = generate_feature_attrs(&handler.features);

    quote! {
        #feature_attrs
        #imports

        /// Generated effect handler
        #[derive(Debug)]
        pub struct #struct_name {
            #(#state_fields),*
        }

        #constructor
        #default_impl
        #trait_impl
    }
}

/// Generate state field definitions
fn generate_state_fields(fields: &[StateField]) -> Vec<TokenStream2> {
    fields.iter().map(|field| {
        let name = &field.name;
        let field_type = &field.field_type;
        quote! { #name: #field_type }
    }).collect()
}

/// Generate constructor methods
fn generate_constructor(struct_name: &Ident, fields: &[StateField], is_mock: bool) -> TokenStream2 {
    if fields.is_empty() {
        // No-state constructor
        quote! {
            impl #struct_name {
                /// Create a new handler instance
                pub fn new() -> Self {
                    Self {}
                }
            }
        }
    } else if is_mock {
        // Mock constructor with configurable state
        let field_names: Vec<_> = fields.iter().map(|f| &f.name).collect();
        let field_types: Vec<_> = fields.iter().map(|f| &f.field_type).collect();
        let default_values: Vec<_> = fields.iter().map(|_| quote! { Default::default() }).collect();

        quote! {
            impl #struct_name {
                /// Create a new mock handler with default values
                pub fn new() -> Self {
                    Self {
                        #(#field_names: #default_values),*
                    }
                }

                /// Create a new mock handler with custom configuration
                pub fn with_config(#(#field_names: #field_types),*) -> Self {
                    Self {
                        #(#field_names),*
                    }
                }

                /// Create a deterministic mock handler for testing
                pub fn new_deterministic() -> Self {
                    Self::new()
                }
            }
        }
    } else {
        // Real constructor
        let field_names: Vec<_> = fields.iter().map(|f| &f.name).collect();
        let default_values: Vec<_> = fields.iter().map(|_| quote! { Default::default() }).collect();

        quote! {
            impl #struct_name {
                /// Create a new handler instance
                pub fn new() -> Self {
                    Self {
                        #(#field_names: #default_values),*
                    }
                }
            }
        }
    }
}

/// Generate Default trait implementation
fn generate_default_impl(struct_name: &Ident, is_mock: bool) -> TokenStream2 {
    if is_mock {
        quote! {
            impl Default for #struct_name {
                fn default() -> Self {
                    Self::new_deterministic()
                }
            }
        }
    } else {
        quote! {
            impl Default for #struct_name {
                fn default() -> Self {
                    Self::new()
                }
            }
        }
    }
}

/// Generate trait implementation
fn generate_trait_impl(
    trait_name: &Path, 
    struct_name: &Ident, 
    methods: &[MethodImpl],
    features: &HandlerFeatures,
) -> TokenStream2 {
    let method_impls: Vec<_> = methods.iter().map(|method| {
        let name = &method.name;
        let params: Vec<_> = method.params.iter().map(|(param_name, param_type)| {
            quote! { #param_name: #param_type }
        }).collect();
        let return_type = &method.return_type;
        let body = &method.body;

        if features.async_trait {
            // For async trait, the body should be an async block or expression
            quote! {
                async fn #name(&self, #(#params),*) -> #return_type #body
            }
        } else {
            quote! {
                fn #name(&self, #(#params),*) -> #return_type #body
            }
        }
    }).collect();

    if features.async_trait {
        quote! {
            #[async_trait::async_trait]
            impl #trait_name for #struct_name {
                #(#method_impls)*
            }
        }
    } else {
        quote! {
            impl #trait_name for #struct_name {
                #(#method_impls)*
            }
        }
    }
}

/// Generate import statements
fn generate_imports(imports: &[Path]) -> TokenStream2 {
    if imports.is_empty() {
        quote! {}
    } else {
        quote! {
            #(use #imports;)*
        }
    }
}

/// Generate feature attributes
fn generate_feature_attrs(features: &HandlerFeatures) -> TokenStream2 {
    let mut attrs = Vec::new();

    if features.disallowed_methods {
        attrs.push(quote! { #[allow(clippy::disallowed_methods)] });
    }
    if features.disallowed_types {
        attrs.push(quote! { #[allow(clippy::disallowed_types)] });
    }

    if attrs.is_empty() {
        quote! {}
    } else {
        quote! { #(#attrs)* }
    }
}

/// Entry point for the effect handlers macro
pub fn aura_effect_handlers_impl(input: TokenStream) -> Result<TokenStream> {
    let config: EffectHandlerConfig = syn::parse(input)?;
    let generated = generate_effect_handlers(config);
    Ok(generated.into())
}