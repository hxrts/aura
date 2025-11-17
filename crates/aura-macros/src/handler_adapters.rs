//! Handler adapter generation macros
//!
//! This module provides macros to generate handler adapter boilerplate for the
//! aura-protocol crate. Handler adapters bridge effect traits to the AuraHandler
//! trait for execution in the stateless effect executor.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::Parse, Ident, LitStr, Token, Type};

/// Input structure for the aura_handler_adapters macro
#[derive(Clone)]
pub struct HandlerAdaptersInput {
    pub adapters: Vec<AdapterSpec>,
}

/// Specification for a single handler adapter
#[derive(Clone)]
pub struct AdapterSpec {
    pub adapter_name: Ident,
    pub trait_name: Type,
    pub effect_type: Ident,
    pub operations: Vec<OperationSpec>,
}

/// Specification for an operation mapping
#[derive(Clone)]
pub struct OperationSpec {
    pub operation_name: LitStr,
    pub method_name: Ident,
    pub param_type: Option<Type>,
    pub return_type: Option<Type>,
}

impl Parse for HandlerAdaptersInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut adapters = Vec::new();

        while !input.is_empty() {
            adapters.push(input.parse()?);
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(HandlerAdaptersInput { adapters })
    }
}

impl Parse for AdapterSpec {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        // Parse: adapter_name: trait_name => effect_type { operations... }
        let adapter_name = input.parse()?;
        input.parse::<Token![:]>()?;
        let trait_name = input.parse()?;
        input.parse::<Token![=>]>()?;
        let effect_type = input.parse()?;

        let content;
        syn::braced!(content in input);

        let mut operations = Vec::new();
        while !content.is_empty() {
            operations.push(content.parse()?);
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }

        Ok(AdapterSpec {
            adapter_name,
            trait_name,
            effect_type,
            operations,
        })
    }
}

impl Parse for OperationSpec {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        // Parse: "operation_name" => method_name(param_type) -> return_type [error_handling]
        let operation_name = input.parse()?;
        input.parse::<Token![=>]>()?;
        let method_name = input.parse()?;

        let param_type = if input.peek(syn::token::Paren) {
            let content;
            syn::parenthesized!(content in input);
            Some(content.parse()?)
        } else {
            None
        };

        let return_type = if input.peek(Token![->]) {
            input.parse::<Token![->]>()?;
            Some(input.parse()?)
        } else {
            None
        };

        Ok(OperationSpec {
            operation_name,
            method_name,
            param_type,
            return_type,
        })
    }
}

/// Generate handler adapter implementations
pub fn aura_handler_adapters_impl(input: TokenStream) -> TokenStream {
    let input = match syn::parse::<HandlerAdaptersInput>(input) {
        Ok(input) => input,
        Err(e) => return e.to_compile_error().into(),
    };

    let mut generated = Vec::new();

    for adapter in input.adapters {
        match generate_adapter(&adapter) {
            Ok(adapter_code) => generated.push(adapter_code),
            Err(e) => return e.to_compile_error().into(),
        }
    }

    quote! {
        use std::{collections::HashMap, error::Error, io, sync::Arc, time::Duration};
        use async_trait::async_trait;
        use serde::{de::DeserializeOwned, Serialize};

        #(#generated)*
    }
    .into()
}

fn generate_adapter(spec: &AdapterSpec) -> Result<proc_macro2::TokenStream, syn::Error> {
    let adapter_name = &spec.adapter_name;
    let trait_name = &spec.trait_name;
    let effect_type = &spec.effect_type;

    let operation_matches = spec.operations.iter().map(|op| {
        let op_name = &op.operation_name;
        let method_name = &op.method_name;

        generate_operation_match(op, op_name, method_name)
    });

    Ok(quote! {
        /// Adapter for `#trait_name` implementations.
        pub struct #adapter_name<T> {
            inner: Arc<T>,
            mode: ExecutionMode,
        }

        impl<T> #adapter_name<T>
        where
            T: #trait_name + Send + Sync + 'static,
        {
            pub fn new(inner: T, mode: ExecutionMode) -> Self {
                Self {
                    inner: Arc::new(inner),
                    mode,
                }
            }

            fn inner(&self) -> &T {
                &self.inner
            }
        }

        #[async_trait]
        impl<T> AuraHandler for #adapter_name<T>
        where
            T: #trait_name + Send + Sync + 'static,
        {
            async fn execute_effect(
                &self,
                _effect_type: EffectType,
                operation: &str,
                params: &[u8],
                _ctx: &AuraContext,
            ) -> Result<Vec<u8>, AuraHandlerError> {
                let effect_type = EffectType::#effect_type;
                Ok(match operation {
                    #(#operation_matches)*
                    _ => {
                        return Err(AuraHandlerError::UnsupportedOperation {
                            effect_type,
                            operation: operation.to_string(),
                        })
                    }
                })
            }

            async fn execute_session(
                &self,
                _session: aura_core::LocalSessionType,
                _ctx: &AuraContext,
            ) -> Result<(), AuraHandlerError> {
                Err(AuraHandlerError::UnsupportedOperation {
                    effect_type: EffectType::#effect_type,
                    operation: "session".to_string(),
                })
            }

            fn supports_effect(&self, effect_type: EffectType) -> bool {
                effect_type == EffectType::#effect_type
            }

            fn execution_mode(&self) -> ExecutionMode {
                self.mode
            }
        }
    })
}

fn generate_operation_match(
    op: &OperationSpec,
    op_name: &syn::LitStr,
    method_name: &syn::Ident,
) -> proc_macro2::TokenStream {
    match (&op.param_type, &op.return_type) {
        (None, None) => {
            // No parameters, no return value
            quote! {
                #op_name => {
                    self.inner().#method_name().await;
                    Vec::new()
                }
            }
        }
        (Some(param_type), None) => {
            // Has parameters, no return value
            quote! {
                #op_name => {
                    let param = deserialize_with_context::<#param_type>(params, effect_type, operation)?;
                    self.inner().#method_name(param).await.map_err(|e| {
                        AuraHandlerError::ExecutionFailed {
                            source: Box::new(e),
                        }
                    })?;
                    Vec::new()
                }
            }
        }
        (None, Some(_return_type)) => {
            // No parameters, has return value
            quote! {
                #op_name => {
                    let result = self.inner().#method_name().await;
                    serialize_with_context(&result, effect_type, operation)?
                }
            }
        }
        (Some(param_type), Some(_return_type)) => {
            // Has parameters and return value
            quote! {
                #op_name => {
                    let param = deserialize_with_context::<#param_type>(params, effect_type, operation)?;
                    let result = self.inner().#method_name(param).await.map_err(|e| {
                        AuraHandlerError::ExecutionFailed {
                            source: Box::new(e),
                        }
                    })?;
                    serialize_with_context(&result, effect_type, operation)?
                }
            }
        }
    }
}
