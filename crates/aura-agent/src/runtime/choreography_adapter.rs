//! Choreography adapter implementation
//!
//! Adapter for integrating with the choreographic programming
//! system from aura-protocol in the authority-centric runtime.
//!
//! ## Role Family Resolution
//!
//! The adapter supports parameterized role families like `Witness[N]` from
//! choreography definitions. Use `with_role_family()` to register role instances:
//!
//! ```ignore
//! let adapter = AuraProtocolAdapter::new(effects, authority_id, self_role, role_map)
//!     .with_role_family("Witness", witness_roles.clone());
//! ```

use async_trait::async_trait;
use aura_core::identifiers::AuthorityId;
use aura_core::util::serialization::{from_slice, to_vec};
use aura_protocol::effects::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError, RoleIndex,
};
use aura_mpst::rumpsteak_aura_choreography::{LabelId, Message, RoleId};
use aura_mpst::ChoreographicAdapterExt;
use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use uuid::Uuid;

/// Adapter for choreography integration
#[derive(Debug)]
pub struct AuraHandlerAdapter {
    authority_id: AuthorityId,
}

impl AuraHandlerAdapter {
    pub fn new(authority_id: AuthorityId) -> Self {
        Self { authority_id }
    }

    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }
}

/// Runtime adapter used by generated choreography runners.
///
/// This adapter implements the `ChoreographicAdapter` trait from rumpsteak-aura,
/// bridging Aura's effect system with the choreographic programming model.
///
/// ## Role Family Support
///
/// For protocols with parameterized roles (e.g., `Witness[N]`), use `with_role_family()`
/// to register role instances that can be resolved during broadcast/collect operations.
#[allow(dead_code)]
#[derive(Debug)]
pub struct AuraProtocolAdapter<E, R>
where
    E: ChoreographicEffects + ?Sized,
    R: RoleId,
{
    effects: Arc<E>,
    authority_id: AuthorityId,
    self_role: R,
    role_map: HashMap<R, AuthorityId>,
    /// Role family registry: maps family names (e.g., "Witness") to role instances
    role_families: HashMap<String, Vec<R>>,
    outbound: VecDeque<Box<dyn Any + Send>>,
    branch_choices: VecDeque<R::Label>,
}

#[allow(dead_code)]
impl<E, R> AuraProtocolAdapter<E, R>
where
    E: ChoreographicEffects + ?Sized,
    R: RoleId,
{
    /// Create a new protocol adapter.
    ///
    /// # Arguments
    ///
    /// * `effects` - The effect system implementation
    /// * `authority_id` - The local authority's ID
    /// * `self_role` - The role this adapter plays in the protocol
    /// * `role_map` - Mapping from roles to authority IDs
    pub fn new(
        effects: Arc<E>,
        authority_id: AuthorityId,
        self_role: R,
        role_map: HashMap<R, AuthorityId>,
    ) -> Self {
        Self {
            effects,
            authority_id,
            self_role,
            role_map,
            role_families: HashMap::new(),
            outbound: VecDeque::new(),
            branch_choices: VecDeque::new(),
        }
    }

    /// Register a role family for broadcast/collect operations.
    ///
    /// This is used for protocols with parameterized roles like `Witness[N]`.
    /// The family name should match the role name in the choreography definition.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // For a choreography with `roles Coordinator, Witness[N]`
    /// let adapter = AuraProtocolAdapter::new(effects, auth_id, role, role_map)
    ///     .with_role_family("Witness", vec![witness0, witness1, witness2]);
    /// ```
    pub fn with_role_family(mut self, family: impl Into<String>, roles: Vec<R>) -> Self {
        self.role_families.insert(family.into(), roles);
        self
    }

    /// Register multiple role families at once.
    pub fn with_role_families(
        mut self,
        families: impl IntoIterator<Item = (String, Vec<R>)>,
    ) -> Self {
        for (family, roles) in families {
            self.role_families.insert(family, roles);
        }
        self
    }

    pub fn push_message<M: Message>(&mut self, message: M) {
        self.outbound.push_back(Box::new(message));
    }

    pub fn push_branch_choice(&mut self, label: R::Label) {
        self.branch_choices.push_back(label);
    }

    pub async fn start_session(&self, session_id: Uuid) -> Result<(), ChoreographyError> {
        let mut roles = Vec::new();
        let self_role = self.map_role(self.self_role)?;
        roles.push(self_role);

        for (role, _) in &self.role_map {
            let mapped = self.map_role(*role)?;
            if mapped != self_role {
                roles.push(mapped);
            }
        }

        self.effects.start_session(session_id, roles).await
    }

    pub async fn end_session(&self) -> Result<(), ChoreographyError> {
        self.effects.end_session().await
    }

    fn map_role(&self, role: R) -> Result<ChoreographicRole, ChoreographyError> {
        let authority_id = if role == self.self_role {
            self.authority_id
        } else {
            *self.role_map.get(&role).ok_or_else(|| {
                ChoreographyError::RoleNotFound {
                    role: ChoreographicRole::new(
                        aura_core::DeviceId::from_uuid(self.authority_id.0),
                        RoleIndex::new(0).expect("role index"),
                    ),
                }
            })?
        };

        let role_index = role.role_index().unwrap_or(0);
        let role_index =
            RoleIndex::new(role_index).ok_or_else(|| ChoreographyError::ProtocolViolation {
                message: format!("invalid role index: {role_index}"),
            })?;

        Ok(ChoreographicRole::new(
            aura_core::DeviceId::from_uuid(authority_id.0),
            role_index,
        ))
    }
}

#[async_trait]
impl<E, R> aura_mpst::rumpsteak_aura_choreography::ChoreographicAdapter
    for AuraProtocolAdapter<E, R>
where
    E: ChoreographicEffects + ?Sized,
    R: RoleId,
{
    type Error = ChoreographyError;
    type Role = R;

    async fn send<M: Message>(&mut self, to: Self::Role, msg: M) -> Result<(), Self::Error> {
        let role = self.map_role(to)?;
        let payload = to_vec(&msg).map_err(|err| ChoreographyError::SerializationFailed {
            reason: err.to_string(),
        })?;
        self.effects.send_to_role_bytes(role, payload).await
    }

    async fn recv<M: Message>(&mut self, from: Self::Role) -> Result<M, Self::Error> {
        let role = self.map_role(from)?;
        let payload = self.effects.receive_from_role_bytes(role).await?;
        from_slice(&payload).map_err(|err| ChoreographyError::DeserializationFailed {
            reason: err.to_string(),
        })
    }

    /// Resolve all instances of a parameterized role family.
    ///
    /// For a choreography with `roles Coordinator, Witness[N]`, calling
    /// `resolve_family("Witness")` returns all registered witness roles.
    fn resolve_family(&self, family: &str) -> Result<Vec<Self::Role>, Self::Error> {
        let roles = self
            .role_families
            .get(family)
            .ok_or_else(|| ChoreographyError::RoleFamilyNotFound {
                family: family.to_string(),
            })?;

        if roles.is_empty() {
            return Err(ChoreographyError::EmptyRoleFamily {
                family: family.to_string(),
            });
        }

        Ok(roles.clone())
    }

    /// Resolve a range of role instances [start, end).
    ///
    /// For a choreography with `Witness[0..3]`, this returns witnesses at indices 0, 1, 2.
    fn resolve_range(
        &self,
        family: &str,
        start: u32,
        end: u32,
    ) -> Result<Vec<Self::Role>, Self::Error> {
        let all_roles = self
            .role_families
            .get(family)
            .ok_or_else(|| ChoreographyError::RoleFamilyNotFound {
                family: family.to_string(),
            })?;

        let start_idx = start as usize;
        let end_idx = end as usize;

        if start_idx >= all_roles.len() || end_idx > all_roles.len() || start_idx >= end_idx {
            return Err(ChoreographyError::InvalidRoleFamilyRange {
                family: family.to_string(),
                start,
                end,
            });
        }

        let roles: Vec<Self::Role> = all_roles[start_idx..end_idx].to_vec();

        if roles.is_empty() {
            return Err(ChoreographyError::EmptyRoleFamily {
                family: family.to_string(),
            });
        }

        Ok(roles)
    }
}

#[async_trait]
impl<E, R> ChoreographicAdapterExt for AuraProtocolAdapter<E, R>
where
    E: ChoreographicEffects + ?Sized,
    R: RoleId,
{
    async fn provide_message<M: Message>(
        &mut self,
        _to: Self::Role,
    ) -> Result<M, Self::Error> {
        let boxed = self.outbound.pop_front().ok_or_else(|| {
            ChoreographyError::ProtocolViolation {
                message: "no queued message for provide_message".to_string(),
            }
        })?;

        boxed.downcast::<M>().map(|msg| *msg).map_err(|_| {
            ChoreographyError::ProtocolViolation {
                message: format!(
                    "queued message type mismatch (expected {})",
                    std::any::type_name::<M>()
                ),
            }
        })
    }

    async fn select_branch<L: LabelId>(&mut self, choices: &[L]) -> Result<L, Self::Error> {
        let choice = self.branch_choices.pop_front().ok_or_else(|| {
            ChoreographyError::ProtocolViolation {
                message: "no queued branch choice for select_branch".to_string(),
            }
        })?;

        let selected = choices
            .iter()
            .copied()
            .find(|label| label.as_str() == choice.as_str());

        selected.ok_or_else(|| ChoreographyError::ProtocolViolation {
            message: "queued branch choice is not valid for this choice".to_string(),
        })
    }
}

/// Public API alias for the choreography adapter.
pub type ChoreographyAdapter = AuraHandlerAdapter;
