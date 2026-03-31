use crate::backend::wg::{
    manage_privileged_service, probe_privileged_service, EngineError, PrivilegedServiceAction,
    PrivilegedServiceStatus,
};

#[derive(Clone, Default)]
pub struct BackendAdminService;

impl BackendAdminService {
    pub fn new() -> Self {
        Self
    }

    pub fn probe_status(&self) -> PrivilegedServiceStatus {
        probe_privileged_service()
    }

    pub fn run_action(&self, action: PrivilegedServiceAction) -> Result<(), EngineError> {
        manage_privileged_service(action)
    }

    pub fn action_verb(&self, action: PrivilegedServiceAction) -> &'static str {
        match action {
            PrivilegedServiceAction::Install => "Installing",
            PrivilegedServiceAction::Repair => "Repairing",
            PrivilegedServiceAction::Remove => "Removing",
        }
    }

    pub fn action_success_message(&self, action: PrivilegedServiceAction) -> &'static str {
        match action {
            PrivilegedServiceAction::Install => "Privileged backend installed",
            PrivilegedServiceAction::Repair => "Privileged backend repaired",
            PrivilegedServiceAction::Remove => "Privileged backend removed",
        }
    }
}
