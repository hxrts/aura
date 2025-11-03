//! AuraMiddleware derive macro implementation

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Attribute, Data, DeriveInput, Fields, Lit, Meta, Type};

pub fn derive_aura_middleware_impl(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate_middleware_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn generate_middleware_impl(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Parse middleware configuration
    let middleware_config = parse_middleware_attributes(&input.attrs)?;

    // Parse fields to identify inner handler and config
    let fields = match &input.data {
        Data::Struct(data) => &data.fields,
        _ => {
            return Err(syn::Error::new_spanned(
                input,
                "AuraMiddleware can only be derived for structs",
            ))
        }
    };

    let field_info = parse_middleware_fields(fields)?;

    // Generate middleware implementation
    let middleware_impl = generate_middleware_trait_impl(
        name,
        &impl_generics,
        &ty_generics,
        where_clause,
        &middleware_config,
        &field_info,
    )?;

    // Generate builder pattern for middleware
    let builder_impl = generate_middleware_builder(name, &field_info)?;

    Ok(quote! {
        #middleware_impl
        #builder_impl
    })
}

#[derive(Default)]
struct MiddlewareConfig {
    handler_trait: String,
    config_type: String,
    async_trait: bool,
}

fn parse_middleware_attributes(attrs: &[Attribute]) -> syn::Result<MiddlewareConfig> {
    let mut config = MiddlewareConfig::default();

    for attr in attrs {
        if attr.path().is_ident("middleware") {
            match &attr.meta {
                Meta::List(meta_list) => {
                    for token in meta_list.tokens.clone().into_iter() {
                        if let Ok(ident) = syn::parse2::<syn::Ident>(quote! { #token }) {
                            if ident == "async_trait" {
                                config.async_trait = true;
                            }
                        }
                    }
                }
                Meta::NameValue(meta_name_value) => {
                    if meta_name_value.path.is_ident("handler") {
                        if let syn::Expr::Lit(expr_lit) = &meta_name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                config.handler_trait = lit_str.value();
                            }
                        }
                    } else if meta_name_value.path.is_ident("config") {
                        if let syn::Expr::Lit(expr_lit) = &meta_name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                config.config_type = lit_str.value();
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

struct MiddlewareField {
    name: syn::Ident,
    field_type: Type,
    is_inner: bool,
    is_config: bool,
}

fn parse_middleware_fields(fields: &Fields) -> syn::Result<Vec<MiddlewareField>> {
    let mut middleware_fields = Vec::new();

    match fields {
        Fields::Named(fields_named) => {
            for field in &fields_named.named {
                let field_name = field.ident.as_ref().unwrap().clone();
                let field_type = field.ty.clone();

                // Determine field role based on name or attributes
                let is_inner = field_name == "inner" || field_name == "handler";
                let is_config =
                    field_name == "config" || field_name.to_string().ends_with("_config");

                middleware_fields.push(MiddlewareField {
                    name: field_name,
                    field_type,
                    is_inner,
                    is_config,
                });
            }
        }
        _ => {
            return Err(syn::Error::new_spanned(
                fields,
                "Only named fields are supported",
            ))
        }
    }

    Ok(middleware_fields)
}

fn generate_middleware_trait_impl(
    name: &syn::Ident,
    _impl_generics: &syn::ImplGenerics,
    _ty_generics: &syn::TypeGenerics,
    where_clause: Option<&syn::WhereClause>,
    config: &MiddlewareConfig,
    field_info: &[MiddlewareField],
) -> syn::Result<TokenStream2> {
    // Find the inner handler field
    let inner_field = field_info
        .iter()
        .find(|f| f.is_inner)
        .ok_or_else(|| syn::Error::new_spanned(name, "Middleware must have an 'inner' field"))?;

    let inner_field_name = &inner_field.name;

    // Generate handler trait name if not specified
    let handler_trait = if config.handler_trait.is_empty() {
        quote! { AuraHandler }
    } else {
        let trait_ident = format_ident!("{}", config.handler_trait);
        quote! { #trait_ident }
    };

    // Generate the trait implementation
    let trait_impl = if config.async_trait {
        quote! {
            #[async_trait::async_trait]
            impl<H: #handler_trait + Send + Sync> #handler_trait for #name<H> #where_clause {
                type Error = H::Error;

                async fn handle_operation(&mut self, operation: Operation) -> Result<OperationResult, Self::Error> {
                    // Pre-processing middleware logic can go here
                    self.before_operation(&operation).await?;

                    // Delegate to inner handler
                    let result = self.#inner_field_name.handle_operation(operation).await;

                    // Post-processing middleware logic can go here
                    self.after_operation(&result).await?;

                    result
                }

                async fn get_state(&self) -> Result<serde_json::Value, Self::Error> {
                    self.#inner_field_name.get_state().await
                }
            }
        }
    } else {
        quote! {
            impl<H: #handler_trait> #handler_trait for #name<H> #where_clause {
                type Error = H::Error;

                fn handle_operation(&mut self, operation: Operation) -> Result<OperationResult, Self::Error> {
                    // Pre-processing middleware logic can go here
                    self.before_operation(&operation)?;

                    // Delegate to inner handler
                    let result = self.#inner_field_name.handle_operation(operation);

                    // Post-processing middleware logic can go here
                    self.after_operation(&result)?;

                    result
                }

                fn get_state(&self) -> Result<serde_json::Value, Self::Error> {
                    self.#inner_field_name.get_state()
                }
            }
        }
    };

    // Generate middleware-specific methods
    let middleware_methods = if config.async_trait {
        quote! {
            impl<H> #name<H> {
                /// Pre-operation hook for middleware processing
                async fn before_operation(&mut self, _operation: &Operation) -> Result<(), H::Error>
                where
                    H: #handler_trait + Send + Sync,
                {
                    // Default implementation - override in actual middleware
                    Ok(())
                }

                /// Post-operation hook for middleware processing
                async fn after_operation(&mut self, _result: &Result<OperationResult, H::Error>) -> Result<(), H::Error>
                where
                    H: #handler_trait + Send + Sync,
                {
                    // Default implementation - override in actual middleware
                    Ok(())
                }

                /// Get reference to inner handler
                pub fn inner(&self) -> &H {
                    &self.#inner_field_name
                }

                /// Get mutable reference to inner handler
                pub fn inner_mut(&mut self) -> &mut H {
                    &mut self.#inner_field_name
                }
            }
        }
    } else {
        quote! {
            impl<H> #name<H> {
                /// Pre-operation hook for middleware processing
                fn before_operation(&mut self, _operation: &Operation) -> Result<(), H::Error>
                where
                    H: #handler_trait,
                {
                    // Default implementation - override in actual middleware
                    Ok(())
                }

                /// Post-operation hook for middleware processing
                fn after_operation(&mut self, _result: &Result<OperationResult, H::Error>) -> Result<(), H::Error>
                where
                    H: #handler_trait,
                {
                    // Default implementation - override in actual middleware
                    Ok(())
                }

                /// Get reference to inner handler
                pub fn inner(&self) -> &H {
                    &self.#inner_field_name
                }

                /// Get mutable reference to inner handler
                pub fn inner_mut(&mut self) -> &mut H {
                    &mut self.#inner_field_name
                }
            }
        }
    };

    Ok(quote! {
        #trait_impl
        #middleware_methods
    })
}

fn generate_middleware_builder(
    name: &syn::Ident,
    field_info: &[MiddlewareField],
) -> syn::Result<TokenStream2> {
    // Find config fields for builder
    let config_fields: Vec<_> = field_info.iter().filter(|f| f.is_config).collect();

    // Generate constructor
    let constructor_params = field_info.iter().map(|field| {
        let field_name = &field.name;
        let field_type = &field.field_type;
        quote! { #field_name: #field_type }
    });

    let constructor_assignments = field_info.iter().map(|field| {
        let field_name = &field.name;
        quote! { #field_name }
    });

    // Generate builder methods for configuration
    let builder_methods = config_fields.iter().map(|field| {
        let field_name = &field.name;
        let method_name = format_ident!("with_{}", field_name);
        let field_type = &field.field_type;

        quote! {
            /// Configure the middleware
            pub fn #method_name(mut self, #field_name: #field_type) -> Self {
                self.#field_name = #field_name;
                self
            }
        }
    });

    Ok(quote! {
        impl<H> #name<H> {
            /// Create a new middleware instance
            pub fn new(#(#constructor_params),*) -> Self {
                Self {
                    #(#constructor_assignments),*
                }
            }

            #(#builder_methods)*
        }

        // Extension trait for fluent middleware composition
        impl<H> aura_types::middleware::LayerExt<H> for #name<H> {
            fn layer<F>(self, f: F) -> F::Output
            where
                F: FnOnce(Self) -> F::Output,
            {
                f(self)
            }
        }
    })
}
