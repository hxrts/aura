//! Choreographic protocol annotation and extension type contracts.

#[path = "protocols/support.rs"]
mod support;

#[allow(clippy::expect_used, missing_docs)]
#[path = "protocols/annotation_extraction.rs"]
mod annotation_extraction;

#[path = "protocols/extension_types.rs"]
mod extension_types;
