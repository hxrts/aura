use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModalFieldDescriptor {
    Direct(FieldId),
    CreateChannelDetails(CreateChannelDetailsField),
    CreateChannelThreshold,
    ThresholdWizard,
    AddDeviceName,
    Capability(CapabilityTier),
}

impl ModalFieldDescriptor {
    pub(super) const fn field_id(self) -> FieldId {
        match self {
            Self::Direct(field_id) => field_id,
            Self::CreateChannelDetails(CreateChannelDetailsField::Name) => {
                FieldId::CreateChannelName
            }
            Self::CreateChannelDetails(CreateChannelDetailsField::Topic) => {
                FieldId::CreateChannelTopic
            }
            Self::CreateChannelThreshold | Self::ThresholdWizard => FieldId::ThresholdInput,
            Self::AddDeviceName => FieldId::DeviceName,
            Self::Capability(CapabilityTier::Full) => FieldId::CapabilityFull,
            Self::Capability(CapabilityTier::Partial) => FieldId::CapabilityPartial,
            Self::Capability(CapabilityTier::Limited) => FieldId::CapabilityLimited,
        }
    }
}

fn single_text_field(
    active: bool,
    descriptor: ModalFieldDescriptor,
    value: &str,
) -> Option<(ModalFieldDescriptor, String)> {
    active.then_some((descriptor, value.to_string()))
}

impl CreateChannelModalState {
    fn field_descriptor(&self) -> Option<ModalFieldDescriptor> {
        match self.step {
            CreateChannelWizardStep::Details => Some(ModalFieldDescriptor::CreateChannelDetails(
                self.active_field,
            )),
            CreateChannelWizardStep::Threshold => {
                Some(ModalFieldDescriptor::CreateChannelThreshold)
            }
            CreateChannelWizardStep::Members => None,
        }
    }

    fn text_value(&self) -> Option<String> {
        match self.field_descriptor()? {
            ModalFieldDescriptor::CreateChannelDetails(CreateChannelDetailsField::Name) => {
                Some(self.name.clone())
            }
            ModalFieldDescriptor::CreateChannelDetails(CreateChannelDetailsField::Topic) => {
                Some(self.topic.clone())
            }
            ModalFieldDescriptor::CreateChannelThreshold => Some(self.threshold.to_string()),
            _ => None,
        }
    }

    fn set_text_value(&mut self, value: String) {
        match self.field_descriptor() {
            Some(ModalFieldDescriptor::CreateChannelDetails(CreateChannelDetailsField::Name)) => {
                self.name = value;
            }
            Some(ModalFieldDescriptor::CreateChannelDetails(CreateChannelDetailsField::Topic)) => {
                self.topic = value;
            }
            Some(ModalFieldDescriptor::CreateChannelThreshold) => {
                self.threshold = value.trim().parse::<u8>().unwrap_or(self.threshold.max(1));
            }
            _ => {}
        }
    }

    fn set_field_value(&mut self, field_id: FieldId, value: String) -> bool {
        if !matches!(self.step, CreateChannelWizardStep::Details) {
            return false;
        }

        self.active_field = match field_id {
            FieldId::CreateChannelName => CreateChannelDetailsField::Name,
            FieldId::CreateChannelTopic => CreateChannelDetailsField::Topic,
            _ => return false,
        };
        self.set_text_value(value);
        true
    }

    fn set_active_field(&mut self, field_id: FieldId) {
        if !matches!(self.step, CreateChannelWizardStep::Details) {
            return;
        }

        self.active_field = match field_id {
            FieldId::CreateChannelTopic => CreateChannelDetailsField::Topic,
            _ => CreateChannelDetailsField::Name,
        };
    }
}

impl ThresholdWizardModalState {
    fn field_descriptor(&self) -> Option<ModalFieldDescriptor> {
        single_text_field(
            matches!(self.step, ThresholdWizardStep::Threshold),
            ModalFieldDescriptor::ThresholdWizard,
            &self.threshold_input,
        )
        .map(|(descriptor, _)| descriptor)
    }

    fn text_value(&self) -> Option<String> {
        single_text_field(
            matches!(self.step, ThresholdWizardStep::Threshold),
            ModalFieldDescriptor::ThresholdWizard,
            &self.threshold_input,
        )
        .map(|(_, value)| value)
    }

    fn set_text_value(&mut self, value: String) {
        if self.field_descriptor().is_some() {
            self.threshold_input = value;
        }
    }
}

impl AddDeviceModalState {
    fn field_descriptor(&self) -> Option<ModalFieldDescriptor> {
        single_text_field(
            self.accepts_name_input(),
            ModalFieldDescriptor::AddDeviceName,
            self.draft_name().unwrap_or_default(),
        )
        .map(|(descriptor, _)| descriptor)
    }

    fn text_value(&self) -> Option<String> {
        single_text_field(
            self.accepts_name_input(),
            ModalFieldDescriptor::AddDeviceName,
            self.draft_name().unwrap_or_default(),
        )
        .map(|(_, value)| value)
    }

    fn set_text_value(&mut self, value: String) {
        self.set_draft_name(value);
    }
}

impl CapabilityConfigModalState {
    fn field_descriptor(&self) -> ModalFieldDescriptor {
        ModalFieldDescriptor::Capability(self.active_tier)
    }

    fn text_value(&self) -> String {
        match self.active_tier {
            CapabilityTier::Full => self.full_caps.clone(),
            CapabilityTier::Partial => self.partial_caps.clone(),
            CapabilityTier::Limited => self.limited_caps.clone(),
        }
    }

    fn set_text_value(&mut self, value: String) {
        match self.active_tier {
            CapabilityTier::Full => self.full_caps = value,
            CapabilityTier::Partial => self.partial_caps = value,
            CapabilityTier::Limited => self.limited_caps = value,
        }
    }
}

impl CreateInvitationModalState {
    const fn normalized_field_id(field_id: FieldId) -> FieldId {
        match field_id {
            FieldId::Nickname
            | FieldId::InvitationReceiverNickname
            | FieldId::InvitationMessage
            | FieldId::InvitationTtl => field_id,
            _ => FieldId::Nickname,
        }
    }

    fn field_descriptor(&self) -> ModalFieldDescriptor {
        ModalFieldDescriptor::Direct(self.active_field)
    }

    fn text_value(&self) -> String {
        match self.active_field {
            FieldId::Nickname => self.nickname.clone(),
            FieldId::InvitationReceiverNickname => self.receiver_nickname.clone(),
            FieldId::InvitationMessage => self.message.clone(),
            FieldId::InvitationTtl => self.ttl_hours.to_string(),
            _ => self.nickname.clone(),
        }
    }

    fn set_text_value(&mut self, value: String) {
        match self.active_field {
            FieldId::Nickname => self.nickname = value,
            FieldId::InvitationReceiverNickname => self.receiver_nickname = value,
            FieldId::InvitationMessage => self.message = value,
            FieldId::InvitationTtl => {
                self.ttl_hours = value.trim().parse::<u64>().unwrap_or(self.ttl_hours.max(1));
            }
            _ => self.nickname = value,
        }
    }

    fn set_field_value(&mut self, field_id: FieldId, value: String) {
        self.active_field = Self::normalized_field_id(field_id);
        self.set_text_value(value);
    }

    fn set_active_field(&mut self, field_id: FieldId) {
        self.active_field = Self::normalized_field_id(field_id);
    }
}

impl ActiveModal {
    pub(super) fn field_descriptor(&self) -> Option<ModalFieldDescriptor> {
        match self {
            Self::CreateInvitation(state) => Some(state.field_descriptor()),
            Self::AcceptContactInvitation(_) | Self::AcceptChannelInvitation(_) => {
                Some(ModalFieldDescriptor::Direct(FieldId::InvitationCode))
            }
            Self::CreateHome(_) => Some(ModalFieldDescriptor::Direct(FieldId::HomeName)),
            Self::EditNickname(_) => Some(ModalFieldDescriptor::Direct(FieldId::Nickname)),
            Self::ImportDeviceEnrollmentCode(_) => {
                Some(ModalFieldDescriptor::Direct(FieldId::DeviceImportCode))
            }
            Self::CreateChannel(state) => state.field_descriptor(),
            Self::GuardianSetup(state) | Self::MfaSetup(state) => state.field_descriptor(),
            Self::AddDevice(state) => state.field_descriptor(),
            Self::CapabilityConfig(state) => Some(state.field_descriptor()),
            Self::EditChannelInfo(_) => {
                Some(ModalFieldDescriptor::Direct(FieldId::CreateChannelName))
            }
            _ => None,
        }
    }

    pub(super) fn text_value(&self) -> Option<String> {
        match self {
            Self::CreateInvitation(state) => Some(state.text_value()),
            Self::AcceptContactInvitation(state)
            | Self::AcceptChannelInvitation(state)
            | Self::CreateHome(state)
            | Self::EditNickname(state)
            | Self::ImportDeviceEnrollmentCode(state) => Some(state.value.clone()),
            Self::CreateChannel(state) => state.text_value(),
            Self::GuardianSetup(state) | Self::MfaSetup(state) => state.text_value(),
            Self::AddDevice(state) => state.text_value(),
            Self::CapabilityConfig(state) => Some(state.text_value()),
            Self::EditChannelInfo(state) => Some(state.name.clone()),
            _ => None,
        }
    }

    pub(super) fn set_text_value(&mut self, value: String) {
        match self {
            Self::CreateInvitation(state) => state.set_text_value(value),
            Self::AcceptContactInvitation(state)
            | Self::AcceptChannelInvitation(state)
            | Self::CreateHome(state)
            | Self::EditNickname(state)
            | Self::ImportDeviceEnrollmentCode(state) => state.value = value,
            Self::CreateChannel(state) => state.set_text_value(value),
            Self::GuardianSetup(state) | Self::MfaSetup(state) => state.set_text_value(value),
            Self::AddDevice(state) => state.set_text_value(value),
            Self::CapabilityConfig(state) => state.set_text_value(value),
            Self::EditChannelInfo(state) => {
                state.name = value;
            }
            _ => {}
        }
    }

    pub(super) fn set_field_value(&mut self, field_id: FieldId, value: String) {
        if let Self::CreateInvitation(state) = self {
            state.set_field_value(field_id, value);
            return;
        }
        if let Self::CreateChannel(state) = self {
            if state.set_field_value(field_id, value.clone()) {
                return;
            }
        }
        if let Self::EditChannelInfo(state) = self {
            match field_id {
                FieldId::CreateChannelName => state.name = value,
                FieldId::CreateChannelTopic => state.topic = value,
                _ => {}
            }
            return;
        }
        self.set_text_value(value);
    }

    pub(super) fn set_active_field(&mut self, field_id: FieldId) {
        if let Self::CreateInvitation(state) = self {
            state.set_active_field(field_id);
            return;
        }
        if let Self::CreateChannel(state) = self {
            state.set_active_field(field_id);
        }
    }

    pub(super) fn accepts_text(&self) -> bool {
        self.field_descriptor().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ActiveModal, AddDeviceModalState, AddDeviceWizardStep, CreateInvitationModalState,
    };
    use aura_app::ui::contract::FieldId;

    #[test]
    fn add_device_name_step_uses_explicit_draft_helpers() {
        let mut state = AddDeviceModalState::default();
        state.push_draft_name_char('L');
        state.push_draft_name_char('a');
        state.push_draft_name_char('p');
        state.push_draft_name_char('t');
        state.push_draft_name_char('o');
        state.push_draft_name_char('p');

        assert_eq!(state.draft_name(), Some("Laptop"));
        assert_eq!(state.commit_draft_name().as_deref(), Some("Laptop"));
        assert_eq!(state.device_name, "Laptop");
        assert_eq!(state.draft_name(), Some(""));

        state.step = AddDeviceWizardStep::ShareCode;
        state.push_draft_name_char('X');
        assert_eq!(state.draft_name(), None);
    }

    #[test]
    fn create_invitation_field_updates_stay_on_supported_fields() {
        let mut modal = ActiveModal::CreateInvitation(CreateInvitationModalState::default());
        modal.set_active_field(FieldId::HomeName);
        assert_eq!(
            modal.field_descriptor().map(|field| field.field_id()),
            Some(FieldId::Nickname)
        );

        modal.set_field_value(FieldId::InvitationMessage, "hello".to_string());
        assert_eq!(modal.text_value().as_deref(), Some("hello"));
    }
}
