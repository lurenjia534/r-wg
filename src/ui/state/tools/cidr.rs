use super::AsyncJobState;
use gpui::{Entity, SharedString, Subscription};
use gpui_component::input::InputState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CidrRequestSnapshot {
    pub(crate) include_text: String,
    pub(crate) exclude_text: String,
}

pub(crate) struct CidrViewModel {
    pub(crate) request: CidrRequestSnapshot,
    pub(crate) normalized_include_text: SharedString,
    pub(crate) normalized_exclude_text: SharedString,
    pub(crate) remaining_text: SharedString,
    pub(crate) allowed_ips_assignment: SharedString,
    pub(crate) summary_rows: Vec<(SharedString, SharedString)>,
}

#[derive(Default)]
pub(crate) struct CidrToolState {
    pub(crate) include_input: Option<Entity<InputState>>,
    pub(crate) exclude_input: Option<Entity<InputState>>,
    pub(crate) include_subscription: Option<Subscription>,
    pub(crate) exclude_subscription: Option<Subscription>,
    pub(crate) generation: u64,
    pub(crate) job: AsyncJobState<CidrViewModel>,
}
