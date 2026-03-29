use aura_app::ui::contract::{ControlId, FieldId};

pub trait RequiredDomId {
    fn required_dom_id(self, context: &'static str) -> &'static str;
}

impl RequiredDomId for Option<&'static str> {
    fn required_dom_id(self, context: &'static str) -> &'static str {
        let Some(id) = self else {
            panic!("{context} must define a web DOM id");
        };
        id
    }
}

impl RequiredDomId for ControlId {
    fn required_dom_id(self, context: &'static str) -> &'static str {
        self.web_dom_id().required_dom_id(context)
    }
}

impl RequiredDomId for FieldId {
    fn required_dom_id(self, context: &'static str) -> &'static str {
        self.web_dom_id().required_dom_id(context)
    }
}

#[must_use]
pub fn control_selector(control_id: ControlId, context: &'static str) -> String {
    format!("#{}", control_id.required_dom_id(context))
}
