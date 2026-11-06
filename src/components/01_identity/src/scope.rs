use contracts::{AuthenticatorError, Principal, PrincipalKind, Role, Scope};

/// The scope table of the authenticator contract. Reads only the principal; there is no worker scope.
pub(crate) fn require(principal: &Principal, scope: Scope) -> Result<(), AuthenticatorError> {
    let session = principal.kind == PrincipalKind::Session;
    let allowed = match scope {
        Scope::Public | Scope::SessionOrDevice => true,
        Scope::SessionOnly => session,
        Scope::Admin => session && principal.role == Role::Admin,
    };
    if allowed {
        Ok(())
    } else {
        Err(AuthenticatorError::Forbidden)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn principal(kind: PrincipalKind, role: Role) -> Principal {
        Principal {
            account_id: Uuid::nil(),
            kind,
            session_or_device_id: Uuid::nil(),
            role,
            label: "test".to_owned(),
        }
    }

    #[test]
    fn scope_table_matches_the_contract_for_every_kind_and_role() {
        // Arrange
        let scopes = [
            Scope::Public,
            Scope::SessionOnly,
            Scope::SessionOrDevice,
            Scope::Admin,
        ];
        let cases = [
            (
                PrincipalKind::Session,
                Role::User,
                [true, true, true, false],
            ),
            (
                PrincipalKind::Session,
                Role::Admin,
                [true, true, true, true],
            ),
            (
                PrincipalKind::Device,
                Role::User,
                [true, false, true, false],
            ),
            (
                PrincipalKind::Device,
                Role::Admin,
                [true, false, true, false],
            ),
        ];

        for (kind, role, expected) in cases {
            // Act
            let allowed = scopes.map(|scope| require(&principal(kind, role), scope).is_ok());

            // Assert
            assert_eq!(allowed, expected, "{kind:?} {role:?}");
        }
    }

    #[test]
    fn a_refused_scope_is_forbidden_not_unauthenticated() {
        // Arrange
        let device = principal(PrincipalKind::Device, Role::User);

        // Act
        let result = require(&device, Scope::Admin);

        // Assert
        assert!(matches!(result, Err(AuthenticatorError::Forbidden)));
    }
}
