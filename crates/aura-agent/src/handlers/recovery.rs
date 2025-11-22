//! Recovery Handlers

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentResult, AuthorityContext};

pub struct RecoveryHandler {
    context: HandlerContext,
}

impl RecoveryHandler {
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;
        Ok(Self {
            context: HandlerContext::new(authority),
        })
    }
}
