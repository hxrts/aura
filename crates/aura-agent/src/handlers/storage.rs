//! Storage Handlers

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentResult, AuthorityContext};

pub struct StorageHandler {
    context: HandlerContext,
}

impl StorageHandler {
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;
        Ok(Self {
            context: HandlerContext::new(authority),
        })
    }
}
