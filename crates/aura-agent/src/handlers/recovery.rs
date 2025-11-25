//! Recovery Handlers

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentResult, AuthorityContext};

pub struct RecoveryHandler {
    #[allow(dead_code)] // Will be used for recovery operations
    context: HandlerContext,
}

impl RecoveryHandler {
    #[allow(dead_code)]
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;
        Ok(Self {
            context: HandlerContext::new(authority),
        })
    }
}
