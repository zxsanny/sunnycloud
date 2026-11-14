//! The last-admin rule (AC-23): every installation keeps at least one account with role admin and state
//! active. One guard serves `update_user` (demotion, disabling) and both deletion requests, inside the
//! transaction that then applies the change.

use contracts::{AccountAdmin, AccountAdminError, AccountState, Role};
use sc_db::DbTx;
use uuid::Uuid;

/// Why the guard did not pass.
#[derive(Debug)]
pub(crate) enum GuardRefusal {
    LastAdmin,
    Port(AccountAdminError),
}

/// Whether a role or state change can take an admin out of the active admins: a demotion or a disabling.
pub(crate) fn removes_admin(role: Option<Role>, state: Option<AccountState>) -> bool {
    role == Some(Role::User) || state == Some(AccountState::Disabled)
}

/// Whether removing `target` leaves an active admin. A target outside the active admins removes none.
fn leaves_an_active_admin(active_admins: &[Uuid], target: Uuid) -> bool {
    !active_admins.contains(&target) || active_admins.iter().any(|admin| *admin != target)
}

/// Locks the active admin rows (always before the target row, so two guarded changes cannot deadlock) and
/// refuses when removing `target` would leave none. The locks hold until `tx` ends, so a second removal waits
/// and then sees the first one's result.
pub(crate) async fn guard_removal(
    accounts: &dyn AccountAdmin,
    tx: &mut DbTx,
    target: Uuid,
) -> Result<(), GuardRefusal> {
    let active_admins = accounts
        .lock_active_admins(tx)
        .await
        .map_err(GuardRefusal::Port)?;
    if leaves_an_active_admin(&active_admins, target) {
        Ok(())
    } else {
        Err(GuardRefusal::LastAdmin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Uuid = Uuid::from_u128(1);
    const B: Uuid = Uuid::from_u128(2);
    const U: Uuid = Uuid::from_u128(3);

    #[test]
    fn removing_the_only_active_admin_is_refused_and_any_other_removal_passes() {
        // Arrange
        let cases: [(&[Uuid], Uuid, bool); 6] = [
            (&[A], A, false),
            (&[A, B], A, true),
            (&[A, B], B, true),
            (&[A], U, true),
            (&[], U, true),
            (&[B], A, true),
        ];

        for (active_admins, target, expected) in cases {
            // Act
            let passes = leaves_an_active_admin(active_admins, target);

            // Assert
            assert_eq!(passes, expected, "{active_admins:?} without {target}");
        }
    }

    #[test]
    fn only_a_demotion_or_a_disabling_is_a_removal() {
        // Arrange
        let cases = [
            (Some(Role::User), None, true),
            (None, Some(AccountState::Disabled), true),
            (Some(Role::Admin), Some(AccountState::Disabled), true),
            (Some(Role::User), Some(AccountState::Active), true),
            (Some(Role::Admin), None, false),
            (None, Some(AccountState::Active), false),
            (Some(Role::Admin), Some(AccountState::Active), false),
            (None, None, false),
        ];

        for (role, state, expected) in cases {
            // Act
            let removal = removes_admin(role, state);

            // Assert
            assert_eq!(removal, expected, "{role:?} {state:?}");
        }
    }
}
