//! Error type generation macros
//!
//! This module provides macros to eliminate boilerplate in error type definitions
//! by auto-generating Display implementations, From conversions, constructor helpers,
//! and other common error handling patterns that appear across 66+ files.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::Parse, Ident, LitStr, Token, Type, Visibility, Attribute};

/// Input specification for the aura_error_types macro
#[derive(Clone)]
pub struct ErrorTypesInput {
    pub visibility: Visibility,
    pub error_name: Ident,
    pub variants: Vec<ErrorVariant>,
    pub derives: Vec<Ident>,
}

/// Specification for a single error variant
#[derive(Clone)]
pub struct ErrorVariant {
    pub attributes: Vec<ErrorAttribute>,
    pub name: Ident,
    pub fields: Vec<ErrorField>,
    pub message_template: LitStr,
    pub category: Option<LitStr>,
    pub code: Option<LitStr>,
}

/// Custom attribute for error variants (like #[category = "storage"])
#[derive(Clone)]
pub struct ErrorAttribute {
    pub name: Ident,
    pub value: LitStr,
}

/// Field in an error variant
#[derive(Clone)]
pub struct ErrorField {
    pub name: Ident,
    pub field_type: Type,
}

impl Parse for ErrorTypesInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let attributes = input.call(Attribute::parse_outer)?;
        let derives = extract_derives(&attributes)?;
        let visibility = input.parse()?;
        input.parse::<Token![enum]>()?;
        let error_name = input.parse()?;
        
        let content;
        syn::braced!(content in input);
        
        let mut variants = Vec::new();
        while !content.is_empty() {
            variants.push(content.parse()?);
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }
        
        Ok(ErrorTypesInput {
            visibility,
            error_name,
            variants,
            derives,
        })
    }
}

impl Parse for ErrorVariant {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut attributes = Vec::new();
        
        // Parse custom attributes like #[category = "storage"]
        while input.peek(Token![#]) {
            input.parse::<Token![#]>()?;
            let content;
            syn::bracketed!(content in input);
            let attr_name = content.parse()?;
            content.parse::<Token![=]>()?;
            let attr_value = content.parse()?;
            attributes.push(ErrorAttribute { name: attr_name, value: attr_value });
        }
        
        let name = input.parse()?;
        
        // Parse fields if present
        let mut fields = Vec::new();
        if input.peek(syn::token::Brace) {
            let content;
            syn::braced!(content in input);
            
            while !content.is_empty() {
                let field_name = content.parse()?;
                content.parse::<Token![:]>()?;
                let field_type = content.parse()?;
                fields.push(ErrorField { name: field_name, field_type });
                
                if content.peek(Token![,]) {
                    content.parse::<Token![,]>()?;
                }
            }
        }
        
        // Parse message template
        input.parse::<Token![=>]>()?;
        let message_template = input.parse()?;
        
        // Extract category from attributes
        let category = attributes.iter()
            .find(|attr| attr.name == "category")
            .map(|attr| attr.value.clone());
        
        // Extract error code from attributes
        let code = attributes.iter()
            .find(|attr| attr.name == "code")
            .map(|attr| attr.value.clone());
        
        Ok(ErrorVariant {
            attributes,
            name,
            fields,
            message_template,
            category,
            code,
        })
    }
}

/// Extract derive macros from attributes
fn extract_derives(attributes: &[Attribute]) -> syn::Result<Vec<Ident>> {
    let mut derives = Vec::new();
    
    for attr in attributes {
        if attr.path().is_ident("derive") {
            attr.parse_args_with(|input: syn::parse::ParseStream| {
                let content;
                syn::parenthesized!(content in input);
                
                while !content.is_empty() {
                    derives.push(content.parse()?);
                    if content.peek(Token![,]) {
                        content.parse::<Token![,]>()?;
                    }
                }
                Ok(())
            })?;
        }
    }
    
    // Add default derives if none specified
    if derives.is_empty() {
        derives.extend([
            Ident::new("Debug", proc_macro2::Span::call_site()),
            Ident::new("Clone", proc_macro2::Span::call_site()),
        ]);
    }
    
    Ok(derives)
}

/// Generate error type implementations with automatic patterns
pub fn aura_error_types_impl(input: TokenStream) -> TokenStream {
    let input = match syn::parse::<ErrorTypesInput>(input) {
        Ok(input) => input,
        Err(e) => return e.to_compile_error().into(),
    };
    
    let error_enum = generate_error_enum(&input);
    let display_impl = generate_display_impl(&input);
    let constructor_impl = generate_constructors(&input);
    let from_conversions = generate_from_conversions(&input);
    let error_trait_impl = generate_error_trait(&input);
    
    quote! {
        #error_enum
        #display_impl
        #constructor_impl
        #from_conversions
        #error_trait_impl
    }.into()
}

fn generate_error_enum(input: &ErrorTypesInput) -> proc_macro2::TokenStream {
    let vis = &input.visibility;
    let error_name = &input.error_name;
    let derives = &input.derives;
    
    let variants = input.variants.iter().map(|variant| {
        let name = &variant.name;
        let fields = &variant.fields;
        
        if fields.is_empty() {
            quote! { #name }
        } else {
            let field_defs = fields.iter().map(|field| {
                let field_name = &field.name;
                let field_type = &field.field_type;
                quote! { #field_name: #field_type }
            });
            
            quote! { #name { #(#field_defs),* } }
        }
    });
    
    quote! {
        #[derive(#(#derives),*)]
        #vis enum #error_name {
            #(#variants),*
        }
    }
}

fn generate_display_impl(input: &ErrorTypesInput) -> proc_macro2::TokenStream {
    let error_name = &input.error_name;
    
    let match_arms = input.variants.iter().map(|variant| {
        let name = &variant.name;
        let template = &variant.message_template;
        let fields = &variant.fields;
        
        if fields.is_empty() {
            quote! {
                Self::#name => write!(f, #template)
            }
        } else {
            let field_names = fields.iter().map(|field| &field.name);
            let field_patterns = field_names.clone();
            
            // Generate interpolation for the template
            let template_str = template.value();
            let interpolated = if template_str.contains('{') {
                // Has interpolation placeholders
                quote! {
                    write!(f, #template, #(#field_names = #field_names),*)
                }
            } else {
                // Simple message
                quote! {
                    write!(f, #template)
                }
            };
            
            quote! {
                Self::#name { #(#field_patterns),* } => #interpolated
            }
        }
    });
    
    quote! {
        impl std::fmt::Display for #error_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    #(#match_arms),*
                }
            }
        }
    }
}

fn generate_constructors(input: &ErrorTypesInput) -> proc_macro2::TokenStream {
    let error_name = &input.error_name;
    
    let constructors = input.variants.iter().map(|variant| {
        let variant_name = &variant.name;
        let fields = &variant.fields;
        
        if fields.is_empty() {
            let constructor_name = Ident::new(
                &variant_name.to_string().to_lowercase(),
                variant_name.span()
            );
            
            quote! {
                /// Create a new error instance
                pub fn #constructor_name() -> Self {
                    Self::#variant_name
                }
            }
        } else {
            let constructor_name = Ident::new(
                &variant_name.to_string().to_lowercase(),
                variant_name.span()
            );
            
            let params = fields.iter().map(|field| {
                let name = &field.name;
                let field_type = &field.field_type;
                quote! { #name: impl Into<#field_type> }
            });
            
            let field_assigns = fields.iter().map(|field| {
                let name = &field.name;
                quote! { #name: #name.into() }
            });
            
            quote! {
                /// Create a new error instance
                pub fn #constructor_name(#(#params),*) -> Self {
                    Self::#variant_name {
                        #(#field_assigns),*
                    }
                }
            }
        }
    });
    
    quote! {
        impl #error_name {
            #(#constructors)*
        }
    }
}

fn generate_from_conversions(input: &ErrorTypesInput) -> proc_macro2::TokenStream {
    let error_name = &input.error_name;
    
    // Generate conversion to AuraError if applicable
    quote! {
        impl From<#error_name> for aura_core::AuraError {
            fn from(err: #error_name) -> Self {
                aura_core::AuraError::internal(err.to_string())
            }
        }
        
        impl std::error::Error for #error_name {}
    }
}

fn generate_error_trait(input: &ErrorTypesInput) -> proc_macro2::TokenStream {
    let error_name = &input.error_name;
    
    let category_arms = input.variants.iter().map(|variant| {
        let variant_name = &variant.name;
        let category = variant.category.as_ref()
            .map(|c| c.value())
            .unwrap_or("general".to_string());
        
        if variant.fields.is_empty() {
            quote! { Self::#variant_name => #category }
        } else {
            quote! { Self::#variant_name { .. } => #category }
        }
    });
    
    let code_arms = input.variants.iter().map(|variant| {
        let variant_name = &variant.name;
        let code = variant.code.as_ref()
            .map(|c| c.value())
            .unwrap_or_else(|| variant_name.to_string().to_lowercase());
        
        if variant.fields.is_empty() {
            quote! { Self::#variant_name => #code }
        } else {
            quote! { Self::#variant_name { .. } => #code }
        }
    });
    
    quote! {
        impl #error_name {
            /// Get the error category for this error
            pub fn category(&self) -> &'static str {
                match self {
                    #(#category_arms),*
                }
            }
            
            /// Get the error code for this error  
            pub fn code(&self) -> &'static str {
                match self {
                    #(#code_arms),*
                }
            }
        }
    }
}