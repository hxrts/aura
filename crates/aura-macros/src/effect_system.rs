//! Effect system execution macros
//!
//! This module provides macros to eliminate boilerplate in effect system implementations
//! by auto-generating the serialize → execute → deserialize patterns that are repeated
//! hundreds of times across trait implementations.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::Parse, Ident, LitStr, Token, Type};

/// Input specification for the aura_effect_implementations macro
#[derive(Clone)]
pub struct EffectImplementationsInput {
    pub implementations: Vec<EffectTraitImpl>,
}

/// Specification for a single effect trait implementation
#[derive(Clone)]
pub struct EffectTraitImpl {
    pub trait_name: Ident,
    pub effect_type: Ident,
    pub error_type: Type,
    pub operations: Vec<EffectOperation>,
}

/// Specification for a single effect operation
#[derive(Clone)]
pub struct EffectOperation {
    pub operation_name: LitStr,
    pub method_name: Ident,
    pub param_type: Option<Type>,
    pub return_type: Option<Type>,
}

impl Parse for EffectImplementationsInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut implementations = Vec::new();

        while !input.is_empty() {
            implementations.push(input.parse()?);
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(EffectImplementationsInput { implementations })
    }
}

impl Parse for EffectTraitImpl {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        // Parse: trait_name: effect_type -> error_type { operations... }
        let trait_name = input.parse()?;
        input.parse::<Token![:]>()?;
        let effect_type = input.parse()?;
        input.parse::<Token![->]>()?;
        let error_type = input.parse()?;

        let content;
        syn::braced!(content in input);

        let mut operations = Vec::new();
        while !content.is_empty() {
            operations.push(content.parse()?);
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }

        Ok(EffectTraitImpl {
            trait_name,
            effect_type,
            error_type,
            operations,
        })
    }
}

impl Parse for EffectOperation {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        // Parse: "operation_name" => method_name(param_type) -> return_type
        let operation_name = input.parse()?;
        input.parse::<Token![=>]>()?;
        let method_name = input.parse()?;

        let param_type = if input.peek(syn::token::Paren) {
            let content;
            syn::parenthesized!(content in input);
            if !content.is_empty() {
                Some(content.parse()?)
            } else {
                None
            }
        } else {
            None
        };

        let return_type = if input.peek(Token![->]) {
            input.parse::<Token![->]>()?;
            Some(input.parse()?)
        } else {
            None
        };

        Ok(EffectOperation {
            operation_name,
            method_name,
            param_type,
            return_type,
        })
    }
}

/// Generate effect trait implementations with automatic execution patterns
pub fn aura_effect_implementations_impl(input: TokenStream) -> TokenStream {
    let input = match syn::parse::<EffectImplementationsInput>(input) {
        Ok(input) => input,
        Err(e) => return e.to_compile_error().into(),
    };

    let mut generated = Vec::new();

    for implementation in input.implementations {
        match generate_trait_impl(&implementation) {
            Ok(impl_code) => generated.push(impl_code),
            Err(e) => return e.to_compile_error().into(),
        }
    }

    quote! {
        use aura_core::AuraError;
        use serde::{de::DeserializeOwned, Serialize};

        /// Helper to serialize parameters for effect execution
        fn serialize_effect_params<T: Serialize>(
            param: &T,
        ) -> Result<Vec<u8>, AuraError> {
            aura_core::util::serialization::to_vec(param).map_err(|e| {
                AuraError::internal(format!("Failed to serialize effect parameters: {}", e))
            })
        }

        /// Helper to deserialize effect results
        fn deserialize_effect_result<T: DeserializeOwned>(
            bytes: &[u8],
        ) -> Result<T, AuraError> {
            aura_core::util::serialization::from_slice(bytes).map_err(|e| {
                AuraError::internal(format!("Failed to deserialize effect result: {}", e))
            })
        }

        #(#generated)*
    }
    .into()
}

fn generate_trait_impl(spec: &EffectTraitImpl) -> Result<proc_macro2::TokenStream, syn::Error> {
    let trait_name = &spec.trait_name;
    let effect_type = &spec.effect_type;
    let error_type = &spec.error_type;

    let methods = spec
        .operations
        .iter()
        .map(|op| generate_method(op, effect_type, error_type));

    Ok(quote! {
        #[async_trait::async_trait]
        impl<T> #trait_name for T
        where
            T: aura_protocol::effects::AuraEffects + ?Sized,
        {
            #(#methods)*
        }
    })
}

fn generate_method(
    op: &EffectOperation,
    effect_type: &Ident,
    error_type: &Type,
) -> proc_macro2::TokenStream {
    let operation_name = &op.operation_name;
    let method_name = &op.method_name;

    match (&op.param_type, &op.return_type) {
        (None, None) => {
            // No parameters, no return value
            quote! {
                async fn #method_name(&self) -> Result<(), #error_type> {
                    self.execute_effect(EffectType::#effect_type, #operation_name, &[])
                        .await
                        .map_err(|e| e.into())?;
                    Ok(())
                }
            }
        }
        (Some(param_type), None) => {
            // Has parameters, no return value
            quote! {
                async fn #method_name(&self, param: #param_type) -> Result<(), #error_type> {
                    let params = serialize_effect_params(&param)?;
                    self.execute_effect(EffectType::#effect_type, #operation_name, &params)
                        .await
                        .map_err(|e| e.into())?;
                    Ok(())
                }
            }
        }
        (None, Some(return_type)) => {
            // No parameters, has return value
            quote! {
                async fn #method_name(&self) -> Result<#return_type, #error_type> {
                    let result = self.execute_effect(EffectType::#effect_type, #operation_name, &[])
                        .await
                        .map_err(|e| e.into())?;
                    deserialize_effect_result(&result).map_err(|e| e.into())
                }
            }
        }
        (Some(param_type), Some(return_type)) => {
            // Has parameters and return value
            quote! {
                async fn #method_name(&self, param: #param_type) -> Result<#return_type, #error_type> {
                    let params = serialize_effect_params(&param)?;
                    let result = self.execute_effect(EffectType::#effect_type, #operation_name, &params)
                        .await
                        .map_err(|e| e.into())?;
                    deserialize_effect_result(&result).map_err(|e| e.into())
                }
            }
        }
    }
}
