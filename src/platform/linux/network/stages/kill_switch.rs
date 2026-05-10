use crate::core::route_plan::{RouteApplyFailureKind, RouteApplyKind, RouteApplyReport};

use super::super::killswitch::{apply_kill_switch, KillSwitchState};
use super::super::policy::PolicyRoutingState;
use super::super::NetworkError;

pub(in crate::platform::linux::network) async fn apply_kill_switch_stage(
    tun_name: &str,
    policy: Option<&PolicyRoutingState>,
    kill_switch_enabled: bool,
    report: &mut RouteApplyReport,
) -> Result<Option<KillSwitchState>, NetworkError> {
    let Some(policy_state) = policy else {
        return Ok(None);
    };

    let kill_switch_fwmark = policy_state.fwmark;
    let kill_switch_v4 = policy_state.v4;
    let kill_switch_v6 = policy_state.v6;
    let kill_switch_item_ids = [
        (kill_switch_v4, "apply:linux:kill_switch_ipv4", "IPv4"),
        (kill_switch_v6, "apply:linux:kill_switch_ipv6", "IPv6"),
    ];

    if !kill_switch_enabled {
        for (enabled, item_id, family_label) in kill_switch_item_ids {
            if !enabled {
                continue;
            }
            report.push_skipped_kind(
                item_id,
                RouteApplyKind::KillSwitch,
                vec![format!(
                    "Linux kill switch is disabled in Settings, so {family_label} traffic is not blocked outside the tunnel."
                )],
            );
        }
        return Ok(None);
    }

    match apply_kill_switch(tun_name, kill_switch_fwmark, kill_switch_v4, kill_switch_v6).await {
        Ok(kill_switch_state) => {
            for (enabled, item_id, family_label) in kill_switch_item_ids {
                if !enabled {
                    continue;
                }
                report.push_applied_kind(
                    item_id,
                    RouteApplyKind::KillSwitch,
                    vec![format!(
                        "Applied Linux kill switch for {family_label} traffic using fwmark 0x{:x}.",
                        kill_switch_fwmark
                    )],
                );
            }
            Ok(Some(kill_switch_state))
        }
        Err(err) => {
            for (enabled, item_id, family_label) in kill_switch_item_ids {
                if !enabled {
                    continue;
                }
                report.push_failed_kind(
                    item_id,
                    RouteApplyKind::KillSwitch,
                    Some(RouteApplyFailureKind::System),
                    vec![format!(
                        "Failed to apply Linux kill switch for {family_label} traffic with fwmark 0x{:x}: {err}",
                        kill_switch_fwmark
                    )],
                );
            }
            report.mark_failed();
            Err(err)
        }
    }
}
