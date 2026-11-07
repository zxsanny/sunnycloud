use std::collections::HashMap;

use contracts::{AccountInfo, AdminUserView, QuotaView};
use uuid::Uuid;

/// `QuotaAdmin::views` answers for up to 500 ids in one round trip; the adapter itself does not cap the slice.
const VIEWS_ID_LIMIT: u32 = 500;

/// The rows one page may ask for: the caller's limit, held to `admin_list_page_size` when set and to the id
/// limit of the usage call.
pub(crate) fn page_limit(requested: u32, configured: Option<u64>) -> u32 {
    let ceiling = configured.map_or(VIEWS_ID_LIMIT, |size| {
        u32::try_from(size).map_or(VIEWS_ID_LIMIT, |size| size.min(VIEWS_ID_LIMIT))
    });
    requested.min(ceiling)
}

/// Accounts in the order 01 gave them, each once, with its usage. 07a omits an account it has no row for
/// (one whose data is already erased); that account shows no use and the installation default.
pub(crate) fn merge(
    accounts: Vec<AccountInfo>,
    usage: &HashMap<Uuid, QuotaView>,
) -> Vec<AdminUserView> {
    accounts
        .into_iter()
        .map(|account| {
            let view = usage.get(&account.id);
            AdminUserView {
                account: account.id,
                role: account.role,
                state: account.state,
                last_login: account.last_login,
                used: view.map_or(0, |view| view.used),
                limit: view.and_then(|view| view.limit_bytes),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use contracts::{AccountStatus, Role};

    use super::*;

    fn info(id: u128) -> AccountInfo {
        AccountInfo {
            id: Uuid::from_u128(id),
            display_name: format!("name {id}"),
            role: Role::User,
            state: AccountStatus::Active,
            last_login: None,
        }
    }

    fn quota(limit: Option<i64>, used: i64) -> QuotaView {
        QuotaView {
            limit_bytes: limit,
            effective_limit: limit.unwrap_or(100),
            used,
            reserved: 7,
        }
    }

    #[test]
    fn every_account_appears_once_in_the_given_order_with_its_usage() {
        // Arrange
        let accounts = vec![info(3), info(1), info(2)];
        let usage = HashMap::from([
            (Uuid::from_u128(1), quota(Some(50), 10)),
            (Uuid::from_u128(2), quota(None, 20)),
            (Uuid::from_u128(3), quota(Some(60), 30)),
        ]);

        // Act
        let merged = merge(accounts, &usage);

        // Assert
        let rows: Vec<(u128, i64, Option<i64>)> = merged
            .iter()
            .map(|view| (view.account.as_u128(), view.used, view.limit))
            .collect();
        assert_eq!(rows, [(3, 30, Some(60)), (1, 10, Some(50)), (2, 20, None)]);
    }

    #[test]
    fn an_account_without_a_usage_entry_shows_no_use_and_the_default_limit() {
        // Arrange
        let usage = HashMap::from([(Uuid::from_u128(1), quota(Some(50), 10))]);

        // Act
        let merged = merge(vec![info(1), info(2)], &usage);

        // Assert
        assert_eq!((merged[1].used, merged[1].limit), (0, None));
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn the_page_never_exceeds_500_rows_or_the_configured_size() {
        // Arrange
        let cases = [
            (20, None, 20),
            (501, None, 500),
            (u32::MAX, None, 500),
            (20, Some(10), 10),
            (5, Some(10), 5),
            (900, Some(1000), 500),
            (900, Some(u64::MAX), 500),
            (0, Some(10), 0),
        ];

        for (requested, configured, expected) in cases {
            // Act
            let limit = page_limit(requested, configured);

            // Assert
            assert_eq!(limit, expected, "{requested} {configured:?}");
        }
    }
}
