// Focused regression tests for backend action guidance.

#[cfg(test)]
mod tests {
    use super::{
        backend_recommended_action, backend_recovery_note, should_show_remove_action,
        should_show_repair_action,
    };
    use crate::ui::state::{BackendDiagnostic, BackendHealth};

    fn diagnostic(health: BackendHealth) -> BackendDiagnostic {
        BackendDiagnostic {
            health,
            detail: "".into(),
            checked_at: None,
        }
    }

    #[test]
    fn running_backend_keeps_repair_and_remove_available() {
        let diagnostic = diagnostic(BackendHealth::Running);

        assert!(should_show_repair_action(&diagnostic));
        assert!(should_show_remove_action(&diagnostic));
    }

    #[test]
    fn running_backend_explains_recovery_actions() {
        let diagnostic = diagnostic(BackendHealth::Running);
        let note = backend_recovery_note(&diagnostic).map(|value| value.to_string());

        assert_eq!(
            backend_recommended_action(&diagnostic),
            "Repair or Remove can stop the running helper before applying system changes."
        );
        assert_eq!(
            note.as_deref(),
            Some(
                "Repair or Remove can stop the running helper first when you need to recover or uninstall it."
            )
        );
    }
}
