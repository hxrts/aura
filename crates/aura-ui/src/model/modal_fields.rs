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
        if matches!(self.step, ThresholdWizardStep::Threshold) {
            Some(ModalFieldDescriptor::ThresholdWizard)
        } else {
            None
        }
    }

    fn text_value(&self) -> Option<String> {
        self.field_descriptor()
            .map(|_| self.threshold_input.clone())
    }

    fn set_text_value(&mut self, value: String) {
        if self.field_descriptor().is_some() {
            self.threshold_input = value;
        }
    }
}

impl AddDeviceModalState {
    fn field_descriptor(&self) -> Option<ModalFieldDescriptor> {
        if matches!(self.step, AddDeviceWizardStep::Name) {
            Some(ModalFieldDescriptor::AddDeviceName)
        } else {
            None
        }
    }

    fn text_value(&self) -> Option<String> {
        self.field_descriptor().map(|_| self.name_input.clone())
    }

    fn set_text_value(&mut self, value: String) {
        if self.field_descriptor().is_some() {
            self.name_input = value;
        }
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

impl ActiveModal {
    pub(super) fn field_descriptor(&self) -> Option<ModalFieldDescriptor> {
        match self {
            Self::CreateInvitation(_) => {
                Some(ModalFieldDescriptor::Direct(FieldId::InvitationReceiver))
            }
            Self::AcceptInvitation(_) => {
                Some(ModalFieldDescriptor::Direct(FieldId::InvitationCode))
            }
            Self::CreateHome(_) => Some(ModalFieldDescriptor::Direct(FieldId::HomeName)),
            Self::SetChannelTopic(_) => {
                Some(ModalFieldDescriptor::Direct(FieldId::CreateChannelTopic))
            }
            Self::EditNickname(_) => Some(ModalFieldDescriptor::Direct(FieldId::Nickname)),
            Self::ImportDeviceEnrollmentCode(_) => {
                Some(ModalFieldDescriptor::Direct(FieldId::DeviceImportCode))
            }
            Self::CreateChannel(state) => state.field_descriptor(),
            Self::GuardianSetup(state) | Self::MfaSetup(state) => state.field_descriptor(),
            Self::AddDevice(state) => state.field_descriptor(),
            Self::CapabilityConfig(state) => Some(state.field_descriptor()),
            _ => None,
        }
    }

    pub(super) fn text_value(&self) -> Option<String> {
        match self {
            Self::CreateInvitation(state) => Some(state.receiver_id.clone()),
            Self::AcceptInvitation(state)
            | Self::CreateHome(state)
            | Self::SetChannelTopic(state)
            | Self::EditNickname(state)
            | Self::ImportDeviceEnrollmentCode(state) => Some(state.value.clone()),
            Self::CreateChannel(state) => state.text_value(),
            Self::GuardianSetup(state) | Self::MfaSetup(state) => state.text_value(),
            Self::AddDevice(state) => state.text_value(),
            Self::CapabilityConfig(state) => Some(state.text_value()),
            _ => None,
        }
    }

    pub(super) fn set_text_value(&mut self, value: String) {
        match self {
            Self::CreateInvitation(state) => state.receiver_id = value,
            Self::AcceptInvitation(state)
            | Self::CreateHome(state)
            | Self::SetChannelTopic(state)
            | Self::EditNickname(state)
            | Self::ImportDeviceEnrollmentCode(state) => state.value = value,
            Self::CreateChannel(state) => state.set_text_value(value),
            Self::GuardianSetup(state) | Self::MfaSetup(state) => state.set_text_value(value),
            Self::AddDevice(state) => state.set_text_value(value),
            Self::CapabilityConfig(state) => state.set_text_value(value),
            _ => {}
        }
    }

    pub(super) fn set_field_value(&mut self, field_id: FieldId, value: String) {
        if let Self::CreateChannel(state) = self {
            if state.set_field_value(field_id, value.clone()) {
                return;
            }
        }
        self.set_text_value(value);
    }

    pub(super) fn set_active_field(&mut self, field_id: FieldId) {
        if let Self::CreateChannel(state) = self {
            state.set_active_field(field_id);
        }
    }

    pub(super) fn accepts_text(&self) -> bool {
        self.field_descriptor().is_some()
    }
}
