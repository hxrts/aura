//! OTA Handlers

use super::shared::{HandlerContext, HandlerUtilities};
use crate::core::{AgentResult, AuthorityContext};

pub struct OtaHandler {
    context: HandlerContext,
}

impl OtaHandler {
    #[allow(dead_code)]
    pub fn new(authority: AuthorityContext) -> AgentResult<Self> {
        HandlerUtilities::validate_authority_context(&authority)?;
        Ok(Self {
            context: HandlerContext::new(authority),
        })
    }
}
