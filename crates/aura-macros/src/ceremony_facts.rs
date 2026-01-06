use proc_macro2::Span;
use quote::quote;
use syn::{parse_macro_input, ItemEnum};

pub fn ceremony_facts_impl(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(Span::call_site(), "ceremony_facts does not take attributes")
            .to_compile_error()
            .into();
    }

    let input = parse_macro_input!(item as ItemEnum);
    let name = &input.ident;

    let expanded = quote! {
        #input

        impl #name {
            /// Extract the ceremony_id for ceremony-scoped facts.
            pub fn ceremony_id(&self) -> Option<&aura_core::identifiers::CeremonyId> {
                match self {
                    Self::CeremonyInitiated { ceremony_id, .. }
                    | Self::CeremonyAcceptanceReceived { ceremony_id, .. }
                    | Self::CeremonyCommitted { ceremony_id, .. }
                    | Self::CeremonyAborted { ceremony_id, .. } => Some(ceremony_id),
                    Self::CeremonySuperseded {
                        superseded_ceremony_id,
                        ..
                    } => Some(superseded_ceremony_id),
                    _ => None,
                }
            }

            /// Extract the timestamp for ceremony-scoped facts.
            pub fn ceremony_timestamp_ms(&self) -> Option<u64> {
                match self {
                    Self::CeremonyInitiated { timestamp_ms, .. }
                    | Self::CeremonyAcceptanceReceived { timestamp_ms, .. }
                    | Self::CeremonyCommitted { timestamp_ms, .. }
                    | Self::CeremonyAborted { timestamp_ms, .. }
                    | Self::CeremonySuperseded { timestamp_ms, .. } => Some(*timestamp_ms),
                    _ => None,
                }
            }
        }
    };

    expanded.into()
}
