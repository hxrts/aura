//! AuraConfig derive macro implementation

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, format_ident};
use syn::{parse_macro_input, DeriveInput, Data, Fields, Meta, Attribute, Lit};

pub fn derive_aura_config_impl(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    match generate_config_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn generate_config_impl(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    
    // Parse attributes for config options
    let config_options = parse_config_attributes(&input.attrs)?;
    
    // Parse fields for validation and default generation
    let fields = match &input.data {
        Data::Struct(data) => &data.fields,
        _ => return Err(syn::Error::new_spanned(input, "AuraConfig can only be derived for structs")),
    };
    
    let field_configs = parse_config_fields(fields)?;
    
    // Generate validation methods
    let validation_impl = generate_validation_methods(&field_configs)?;
    
    // Generate loading methods
    let loading_impl = generate_loading_methods(&config_options)?;
    
    // Generate merging methods
    let merging_impl = generate_merging_methods(&field_configs)?;
    
    // Generate defaults methods
    let defaults_impl = generate_defaults_methods(&field_configs)?;
    
    Ok(quote! {
        impl #impl_generics aura_types::config::AuraConfig for #name #ty_generics #where_clause {
            type Error = aura_types::AuraError;
            
            fn load_from_file(path: &std::path::Path) -> Result<Self, Self::Error> {
                #loading_impl
            }
            
            fn save_to_file(&self, path: &std::path::Path) -> Result<(), Self::Error> {
                let content = self.serialize()?;
                std::fs::write(path, content)
                    .map_err(|e| aura_types::AuraError::config_failed(format!("Failed to write config file: {}", e)))
            }
            
            fn merge_with_env(&mut self) -> Result<(), Self::Error> {
                // Generate environment variable loading logic
                Ok(())
            }
            
            fn validate(&self) -> Result<(), Self::Error> {
                #validation_impl
                Ok(())
            }
            
            fn merge_with(&mut self, other: &Self) -> Result<(), Self::Error> {
                #merging_impl
                Ok(())
            }
            
            fn defaults() -> Self {
                #defaults_impl
            }
        }
        
        impl #impl_generics #name #ty_generics #where_clause {
            /// Serialize configuration to string format
            fn serialize(&self) -> Result<String, aura_types::AuraError> {
                // Generate serialization based on file format
                serde_json::to_string_pretty(self)
                    .map_err(|e| aura_types::AuraError::config_failed(format!("Serialization failed: {}", e)))
            }
            
            /// Deserialize configuration from string
            fn deserialize(content: &str) -> Result<Self, aura_types::AuraError> {
                // Generate deserialization based on file format
                serde_json::from_str(content)
                    .map_err(|e| aura_types::AuraError::config_failed(format!("Deserialization failed: {}", e)))
            }
        }
    })
}

#[derive(Default)]
struct ConfigOptions {
    validate: bool,
    merge: bool,
    defaults: bool,
    file_format: String,
}

fn parse_config_attributes(attrs: &[Attribute]) -> syn::Result<ConfigOptions> {
    let mut options = ConfigOptions {
        file_format: "json".to_string(),
        ..Default::default()
    };
    
    for attr in attrs {
        if attr.path().is_ident("config") {
            match &attr.meta {
                Meta::List(meta_list) => {
                    for token in meta_list.tokens.clone().into_iter() {
                        if let Ok(ident) = syn::parse2::<syn::Ident>(quote! { #token }) {
                            match ident.to_string().as_str() {
                                "validate" => options.validate = true,
                                "merge" => options.merge = true,
                                "defaults" => options.defaults = true,
                                _ => {}
                            }
                        }
                    }
                }
                Meta::NameValue(meta_name_value) => {
                    if meta_name_value.path.is_ident("file_format") {
                        if let syn::Expr::Lit(expr_lit) = &meta_name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                options.file_format = lit_str.value();
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    
    Ok(options)
}

struct ConfigField {
    name: syn::Ident,
    required: bool,
    default_value: Option<String>,
    validator: Option<String>,
    range: Option<String>,
}

fn parse_config_fields(fields: &Fields) -> syn::Result<Vec<ConfigField>> {
    let mut config_fields = Vec::new();
    
    match fields {
        Fields::Named(fields_named) => {
            for field in &fields_named.named {
                let field_name = field.ident.as_ref().unwrap().clone();
                let field_config = parse_config_field_attributes(&field.attrs)?;
                
                config_fields.push(ConfigField {
                    name: field_name,
                    required: field_config.0,
                    default_value: field_config.1,
                    validator: field_config.2,
                    range: field_config.3,
                });
            }
        }
        _ => return Err(syn::Error::new_spanned(fields, "Only named fields are supported")),
    }
    
    Ok(config_fields)
}

fn parse_config_field_attributes(attrs: &[Attribute]) -> syn::Result<(bool, Option<String>, Option<String>, Option<String>)> {
    let mut required = false;
    let mut default_value = None;
    let mut validator = None;
    let mut range = None;
    
    for attr in attrs {
        if attr.path().is_ident("config") {
            match &attr.meta {
                Meta::List(meta_list) => {
                    for token in meta_list.tokens.clone().into_iter() {
                        if let Ok(ident) = syn::parse2::<syn::Ident>(quote! { #token }) {
                            if ident == "required" {
                                required = true;
                            }
                        }
                    }
                }
                Meta::NameValue(meta_name_value) => {
                    if meta_name_value.path.is_ident("default") {
                        if let syn::Expr::Lit(expr_lit) = &meta_name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                default_value = Some(lit_str.value());
                            }
                        }
                    } else if meta_name_value.path.is_ident("validate") {
                        if let syn::Expr::Lit(expr_lit) = &meta_name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                validator = Some(lit_str.value());
                            }
                        }
                    } else if meta_name_value.path.is_ident("range") {
                        if let syn::Expr::Lit(expr_lit) = &meta_name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                range = Some(lit_str.value());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    
    Ok((required, default_value, validator, range))
}

fn generate_validation_methods(field_configs: &[ConfigField]) -> syn::Result<TokenStream2> {
    let validations = field_configs.iter().map(|field| {
        let field_name = &field.name;
        let field_name_str = field_name.to_string();
        
        let mut validation_checks = Vec::new();
        
        // Required field validation
        if field.required {
            validation_checks.push(quote! {
                // Add required field validation if needed
            });
        }
        
        // Range validation
        if let Some(_range) = &field.range {
            validation_checks.push(quote! {
                // Add range validation logic
            });
        }
        
        // Custom validator
        if let Some(validator) = &field.validator {
            let validator_ident = format_ident!("{}", validator);
            validation_checks.push(quote! {
                #validator_ident(&self.#field_name)
                    .map_err(|e| aura_types::AuraError::config_failed(
                        format!("Validation failed for {}: {}", #field_name_str, e)
                    ))?;
            });
        }
        
        quote! {
            #(#validation_checks)*
        }
    });
    
    Ok(quote! {
        #(#validations)*
    })
}

fn generate_loading_methods(config_options: &ConfigOptions) -> syn::Result<TokenStream2> {
    let deserializer = match config_options.file_format.as_str() {
        "toml" => quote! {
            let content = std::fs::read_to_string(path)
                .map_err(|e| aura_types::AuraError::config_failed(format!("Failed to read config file: {}", e)))?;
            toml::from_str(&content)
                .map_err(|e| aura_types::AuraError::config_failed(format!("Failed to parse TOML: {}", e)))
        },
        "json" => quote! {
            let content = std::fs::read_to_string(path)
                .map_err(|e| aura_types::AuraError::config_failed(format!("Failed to read config file: {}", e)))?;
            serde_json::from_str(&content)
                .map_err(|e| aura_types::AuraError::config_failed(format!("Failed to parse JSON: {}", e)))
        },
        _ => quote! {
            let content = std::fs::read_to_string(path)
                .map_err(|e| aura_types::AuraError::config_failed(format!("Failed to read config file: {}", e)))?;
            Self::deserialize(&content)
        },
    };
    
    Ok(deserializer)
}

fn generate_merging_methods(_field_configs: &[ConfigField]) -> syn::Result<TokenStream2> {
    // For now, generate a simple merge that overwrites with other's values
    Ok(quote! {
        // Simple merge implementation - can be enhanced per field
        *self = other.clone();
    })
}

fn generate_defaults_methods(field_configs: &[ConfigField]) -> syn::Result<TokenStream2> {
    let default_assignments = field_configs.iter().map(|field| {
        let field_name = &field.name;
        
        if let Some(default_val) = &field.default_value {
            // Try to parse the default value appropriately
            quote! {
                #field_name: #default_val.parse().unwrap_or_default(),
            }
        } else {
            quote! {
                #field_name: Default::default(),
            }
        }
    });
    
    Ok(quote! {
        Self {
            #(#default_assignments)*
        }
    })
}