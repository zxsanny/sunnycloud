use contracts::{Actor, PrincipalKind, Role};

use crate::error::AdminCallError;

/// A signed-in admin passes only with a session principal of role admin: a device token never does, whatever
/// its account's role. The operator is the trusted CLI.
pub(crate) fn authorize(actor: &Actor) -> Result<(), AdminCallError> {
    let permitted = match actor {
        Actor::Operator => true,
        Actor::Admin(principal) => {
            principal.kind == PrincipalKind::Session && principal.role == Role::Admin
        }
    };
    if permitted {
        Ok(())
    } else {
        Err(AdminCallError::Forbidden)
    }
}

#[cfg(test)]
mod tests {
    use contracts::Principal;
    use uuid::Uuid;

    use super::*;

    fn principal(kind: PrincipalKind, role: Role) -> Actor {
        Actor::Admin(Principal {
            account_id: Uuid::nil(),
            kind,
            session_or_device_id: Uuid::nil(),
            role,
            label: String::new(),
        })
    }

    #[test]
    fn only_an_admin_session_or_the_operator_passes() {
        // Arrange
        let cases = [
            (principal(PrincipalKind::Session, Role::Admin), true),
            (principal(PrincipalKind::Session, Role::User), false),
            (principal(PrincipalKind::Device, Role::Admin), false),
            (principal(PrincipalKind::Device, Role::User), false),
            (Actor::Operator, true),
        ];

        for (actor, expected) in cases {
            // Act
            let result = authorize(&actor);

            // Assert
            assert_eq!(result.is_ok(), expected, "{actor:?}");
            if !expected {
                assert!(matches!(result, Err(AdminCallError::Forbidden)));
            }
        }
    }
}
