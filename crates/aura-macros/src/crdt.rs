//! CrdtState derive macro implementation

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, format_ident};
use syn::{parse_macro_input, DeriveInput, Data, Fields, Meta, Attribute, Lit};

pub fn derive_crdt_state_impl(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    match generate_crdt_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn generate_crdt_impl(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let generics = &input.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    
    // Parse CRDT configuration from attributes
    let crdt_config = parse_crdt_attributes(&input.attrs)?;
    
    // Parse field configurations
    let fields = match &input.data {
        Data::Struct(data) => &data.fields,
        _ => return Err(syn::Error::new_spanned(input, "CrdtState can only be derived for structs")),
    };
    
    let field_configs = parse_crdt_fields(fields)?;
    
    // Generate CRDT implementation based on backend (automerge, yrs, etc.)
    let crdt_impl = match crdt_config.backend.as_str() {
        "automerge" => generate_automerge_impl(name, &impl_generics, &ty_generics, where_clause, &field_configs)?,
        _ => return Err(syn::Error::new_spanned(input, "Unsupported CRDT backend")),
    };
    
    Ok(crdt_impl)
}

#[derive(Default)]
struct CrdtConfig {
    backend: String,
    conflict_resolution: String,
}

fn parse_crdt_attributes(attrs: &[Attribute]) -> syn::Result<CrdtConfig> {
    let mut config = CrdtConfig {
        backend: "automerge".to_string(),
        conflict_resolution: "last_write_wins".to_string(),
    };
    
    for attr in attrs {
        if attr.path().is_ident("crdt") {
            match &attr.meta {
                Meta::List(meta_list) => {
                    for token in meta_list.tokens.clone().into_iter() {
                        if let Ok(ident) = syn::parse2::<syn::Ident>(quote! { #token }) {
                            if ident == "automerge" {
                                config.backend = "automerge".to_string();
                            }
                        }
                    }
                }
                Meta::NameValue(meta_name_value) => {
                    if meta_name_value.path.is_ident("conflict_resolution") {
                        if let syn::Expr::Lit(expr_lit) = &meta_name_value.value {
                            if let Lit::Str(lit_str) = &expr_lit.lit {
                                config.conflict_resolution = lit_str.value();
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

struct CrdtField {
    name: syn::Ident,
    crdt_type: CrdtFieldType,
}

#[derive(Debug)]
enum CrdtFieldType {
    Counter,
    Set,
    Map,
    Text,
    Regular,
}

fn parse_crdt_fields(fields: &Fields) -> syn::Result<Vec<CrdtField>> {
    let mut crdt_fields = Vec::new();
    
    match fields {
        Fields::Named(fields_named) => {
            for field in &fields_named.named {
                let field_name = field.ident.as_ref().unwrap().clone();
                let crdt_type = parse_crdt_field_attributes(&field.attrs)?;
                
                crdt_fields.push(CrdtField {
                    name: field_name,
                    crdt_type,
                });
            }
        }
        _ => return Err(syn::Error::new_spanned(fields, "Only named fields are supported")),
    }
    
    Ok(crdt_fields)
}

fn parse_crdt_field_attributes(attrs: &[Attribute]) -> syn::Result<CrdtFieldType> {
    for attr in attrs {
        if attr.path().is_ident("crdt") {
            if let Meta::List(meta_list) = &attr.meta {
                for token in meta_list.tokens.clone().into_iter() {
                    if let Ok(ident) = syn::parse2::<syn::Ident>(quote! { #token }) {
                        match ident.to_string().as_str() {
                            "counter" => return Ok(CrdtFieldType::Counter),
                            "set" => return Ok(CrdtFieldType::Set),
                            "map" => return Ok(CrdtFieldType::Map),
                            "text" => return Ok(CrdtFieldType::Text),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    
    Ok(CrdtFieldType::Regular)
}

fn generate_automerge_impl(
    name: &syn::Ident,
    impl_generics: &syn::ImplGenerics,
    ty_generics: &syn::TypeGenerics,
    where_clause: Option<&syn::WhereClause>,
    field_configs: &[CrdtField],
) -> syn::Result<TokenStream2> {
    
    // Generate field operations for each CRDT field type
    let field_operations = field_configs.iter().map(|field| {
        let field_name = &field.name;
        let field_name_str = field_name.to_string();
        
        match field.crdt_type {
            CrdtFieldType::Counter => {
                let increment_method = format_ident!("increment_{}", field_name);
                let get_method = format_ident!("get_{}", field_name);
                
                quote! {
                    /// Increment the counter field
                    pub fn #increment_method(&mut self) -> Result<Vec<automerge::Change>, aura_types::AuraError> {
                        let current = self.doc.get(automerge::ROOT, #field_name_str)
                            .ok()
                            .and_then(|opt| opt.map(|(v, _)| v))
                            .and_then(|v| v.to_u64())
                            .unwrap_or(0);
                        
                        self.doc.put(automerge::ROOT, #field_name_str, current + 1)
                            .map_err(|e| aura_types::AuraError::crdt_failed(format!("Failed to increment {}: {}", #field_name_str, e)))?;
                        
                        Ok(self.doc.get_changes(&[]).into_iter().cloned().collect())
                    }
                    
                    /// Get the current counter value
                    pub fn #get_method(&self) -> u64 {
                        self.doc.get(automerge::ROOT, #field_name_str)
                            .ok()
                            .and_then(|opt| opt.map(|(v, _)| v))
                            .and_then(|v| v.to_u64())
                            .unwrap_or(0)
                    }
                }
            }
            CrdtFieldType::Set => {
                let add_method = format_ident!("add_to_{}", field_name);
                let remove_method = format_ident!("remove_from_{}", field_name);
                let contains_method = format_ident!("{}_contains", field_name);
                
                quote! {
                    /// Add item to the set
                    pub fn #add_method<T: serde::Serialize>(&mut self, item: T) -> Result<Vec<automerge::Change>, aura_types::AuraError> {
                        let set_obj = match self.doc.get(automerge::ROOT, #field_name_str) {
                            Ok(Some((_, obj_id))) => obj_id,
                            _ => {
                                // Create set if it doesn't exist
                                self.doc.put_object(automerge::ROOT, #field_name_str, automerge::ObjType::List)
                                    .map_err(|e| aura_types::AuraError::crdt_failed(format!("Failed to create set {}: {}", #field_name_str, e)))?
                            }
                        };
                        
                        let item_json = serde_json::to_string(&item)
                            .map_err(|e| aura_types::AuraError::crdt_failed(format!("Failed to serialize item: {}", e)))?;
                        
                        let list_len = self.doc.length(&set_obj);
                        self.doc.insert(&set_obj, list_len, item_json)
                            .map_err(|e| aura_types::AuraError::crdt_failed(format!("Failed to add to set {}: {}", #field_name_str, e)))?;
                        
                        Ok(self.doc.get_changes(&[]).into_iter().cloned().collect())
                    }
                    
                    /// Remove item from the set
                    pub fn #remove_method<T: serde::Serialize>(&mut self, item: T) -> Result<Vec<automerge::Change>, aura_types::AuraError> {
                        // Implementation for set removal (tombstone pattern)
                        todo!("Implement set removal")
                    }
                    
                    /// Check if set contains item
                    pub fn #contains_method<T: serde::Serialize>(&self, item: T) -> bool {
                        // Implementation for set membership check
                        false
                    }
                }
            }
            CrdtFieldType::Map => {
                let put_method = format_ident!("put_in_{}", field_name);
                let get_method = format_ident!("get_from_{}", field_name);
                
                quote! {
                    /// Put key-value pair in the map
                    pub fn #put_method<K: AsRef<str>, V: serde::Serialize>(&mut self, key: K, value: V) -> Result<Vec<automerge::Change>, aura_types::AuraError> {
                        let map_obj = match self.doc.get(automerge::ROOT, #field_name_str) {
                            Ok(Some((_, obj_id))) => obj_id,
                            _ => {
                                // Create map if it doesn't exist
                                self.doc.put_object(automerge::ROOT, #field_name_str, automerge::ObjType::Map)
                                    .map_err(|e| aura_types::AuraError::crdt_failed(format!("Failed to create map {}: {}", #field_name_str, e)))?
                            }
                        };
                        
                        let value_json = serde_json::to_string(&value)
                            .map_err(|e| aura_types::AuraError::crdt_failed(format!("Failed to serialize value: {}", e)))?;
                        
                        self.doc.put(&map_obj, key.as_ref(), value_json)
                            .map_err(|e| aura_types::AuraError::crdt_failed(format!("Failed to put in map {}: {}", #field_name_str, e)))?;
                        
                        Ok(self.doc.get_changes(&[]).into_iter().cloned().collect())
                    }
                    
                    /// Get value from the map
                    pub fn #get_method<K: AsRef<str>>(&self, key: K) -> Option<serde_json::Value> {
                        // Implementation for map value retrieval
                        None
                    }
                }
            }
            _ => quote! {
                // Regular field - no special CRDT operations
            }
        }
    });
    
    Ok(quote! {
        impl #impl_generics aura_types::crdt::CrdtState for #name #ty_generics #where_clause {
            type Change = automerge::Change;
            type StateId = automerge::ChangeHash;
            type Error = aura_types::AuraError;
            
            fn apply_changes(&mut self, changes: impl IntoIterator<Item = Self::Change>) -> Result<(), Self::Error> {
                self.doc.apply_changes(changes)
                    .map_err(|e| aura_types::AuraError::crdt_failed(format!("Failed to apply changes: {}", e)))
            }
            
            fn get_changes(&self, since: &[Self::StateId]) -> Vec<Self::Change> {
                self.doc.get_changes(since).into_iter().cloned().collect()
            }
            
            fn get_state_id(&self) -> Vec<Self::StateId> {
                self.doc.get_heads()
            }
            
            fn merge_with(&mut self, other: &Self) -> Result<Vec<Self::Change>, Self::Error> {
                let other_changes = other.get_changes(&[]);
                self.apply_changes(other_changes.clone())?;
                Ok(other_changes)
            }
            
            fn save(&self) -> Result<Vec<u8>, Self::Error> {
                Ok(self.doc.save())
            }
            
            fn load(data: &[u8]) -> Result<Self, Self::Error> {
                let doc = automerge::Automerge::load(data)
                    .map_err(|e| aura_types::AuraError::crdt_failed(format!("Failed to load CRDT state: {}", e)))?;
                
                // Convert to AutoCommit
                let mut auto_commit = automerge::AutoCommit::new();
                auto_commit.apply_changes(doc.get_changes(&[]).into_iter().cloned())
                    .map_err(|e| aura_types::AuraError::crdt_failed(format!("Failed to apply changes: {}", e)))?;
                
                Ok(Self {
                    doc: auto_commit,
                })
            }
        }
        
        impl #impl_generics #name #ty_generics #where_clause {
            /// Create a new CRDT state instance
            pub fn new() -> Result<Self, aura_types::AuraError> {
                let doc = automerge::AutoCommit::new();
                Ok(Self { doc })
            }
            
            /// Get the underlying Automerge document
            pub fn document(&self) -> &automerge::AutoCommit {
                &self.doc
            }
            
            /// Get mutable reference to the underlying document
            pub fn document_mut(&mut self) -> &mut automerge::AutoCommit {
                &mut self.doc
            }
            
            #(#field_operations)*
        }
    })
}