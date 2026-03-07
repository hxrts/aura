use super::*;

pub(super) fn validate_guardian_setup_inputs(
    guardians: &[AuthorityId],
    threshold: u16,
) -> AgentResult<()> {
    use crate::core::AgentError;

    if guardians.len() != 3 {
        return Err(AgentError::invalid(
            "Guardian setup requires exactly three guardians".to_string(),
        ));
    }

    if threshold == 0 {
        return Err(AgentError::invalid(
            "Guardian setup threshold must be at least 1".to_string(),
        ));
    }

    if threshold as usize > guardians.len() {
        return Err(AgentError::invalid(format!(
            "Guardian setup threshold {} exceeds guardian count {}",
            threshold,
            guardians.len()
        )));
    }

    Ok(())
}

pub(super) fn build_guardian_setup_completion(
    setup_id: &str,
    threshold: u16,
    acceptances: Vec<GuardianAcceptance>,
) -> SetupCompletion {
    let accepted_guardians: Vec<AuthorityId> = acceptances
        .iter()
        .filter(|acceptance| acceptance.accepted)
        .map(|acceptance| acceptance.guardian_id)
        .collect();

    let guardian_set = GuardianSet::new(
        accepted_guardians
            .iter()
            .copied()
            .map(GuardianProfile::new)
            .collect(),
    );

    SetupCompletion {
        setup_id: setup_id.to_string(),
        success: accepted_guardians.len() >= threshold as usize,
        guardian_set,
        threshold,
        encrypted_shares: Vec::new(),
        public_key_package: Vec::new(),
    }
}
