//! The email rule at invite redemption (01 §5, AZ-3685 AC-3, IT-01-6, FT-N-22, NFT-SEC-02): an unverified or
//! relay address never creates or links an account, email is never a lookup key, a verified address that is not
//! on the allowlist refuses the redemption (owner decision 2026-11-13), and with no allowlist no email is read.

mod support;

use contracts::{SignInError, SignInOutcome};
use support::registration::{INVITE, Rig, sign_in};
use support::sign_in::{GITHUB, OIDC};
use uuid::Uuid;

const POOL: u32 = 5;
const VICTIM: &str = "victim@example.test";
const RELAY: &str = "x1@privaterelay.appleid.com";

async fn allowlisted() -> Rig {
    Rig::new(
        POOL,
        |config| config.email_allowlist = vec![VICTIM.to_owned()],
        Vec::new(),
    )
    .await
}

/// The victim's account, its identity carrying the victim's verified email.
async fn victim_account(rig: &mut Rig) -> Uuid {
    let account = rig.world.known_identity(OIDC, "victim-sub").await;
    sqlx::query("UPDATE identities SET email = $1, email_verified = true WHERE account_id = $2")
        .bind(VICTIM)
        .bind(account)
        .execute(&mut rig.world.conn)
        .await
        .expect("store the victim's email");
    account
}

fn invitation_required(result: &Result<SignInOutcome, SignInError>) -> bool {
    matches!(result, Err(SignInError::InvitationRequired))
}

#[tokio::test]
async fn unverified_relay_and_uninvited_verified_emails_create_and_link_nothing_and_only_an_invite_creates()
 {
    // Arrange
    let mut rig = allowlisted().await;
    let victim = victim_account(&mut rig).await;
    rig.member_invite(INVITE).await;
    let before = rig.world.counts().await;

    // Act
    let oidc_unverified = sign_in(&rig.world, OIDC, "attacker-1", None, |grant| {
        grant.email = Some((VICTIM.to_owned(), false));
    })
    .await;
    rig.world.idp.behave(|behaviour| {
        behaviour.github_emails = vec![(VICTIM.to_owned(), true, false)];
    });
    let github_unverified = sign_in(&rig.world, GITHUB, "6661", None, |_| {}).await;
    let relay = sign_in(&rig.world, OIDC, "attacker-3", None, |grant| {
        grant.email = Some((RELAY.to_owned(), true));
    })
    .await;
    let verified_without_invite = sign_in(&rig.world, OIDC, "attacker-4", None, |grant| {
        grant.email = Some((VICTIM.to_owned(), true));
    })
    .await;
    let refused = rig.world.counts().await;
    let verified_with_invite = sign_in(&rig.world, OIDC, "newcomer", Some(INVITE), |grant| {
        grant.email = Some((VICTIM.to_owned(), true));
    })
    .await;

    // Assert
    for result in [
        &oidc_unverified,
        &github_unverified,
        &relay,
        &verified_without_invite,
    ] {
        assert!(invitation_required(result), "{result:?}");
    }
    assert_eq!(
        (refused.accounts, refused.identities),
        (before.accounts, before.identities)
    );
    assert!(
        matches!(verified_with_invite, Ok(SignInOutcome::Session { .. })),
        "{verified_with_invite:?}"
    );
    let (newcomer, _, _) = rig.account_of(OIDC, "newcomer").await.expect("new account");
    assert_ne!(newcomer, victim, "the email found no account");
    let after = rig.world.counts().await;
    assert_eq!(
        (after.accounts, after.identities),
        (before.accounts + 1, before.identities + 1)
    );
    let victim_identities: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identities WHERE account_id = $1")
            .bind(victim)
            .fetch_one(&mut rig.world.conn)
            .await
            .expect("count the victim's identities");
    assert_eq!(victim_identities, 1, "nothing was linked to the victim");
    rig.finish().await;
}

#[tokio::test]
async fn with_an_allowlist_an_invite_is_refused_for_an_unlisted_unverified_relay_or_missing_email()
{
    // Arrange
    let mut rig = allowlisted().await;
    let invite = rig.member_invite(INVITE).await;
    let emails = [
        Some(("other@example.test", true)),
        Some((VICTIM, false)),
        Some((RELAY, true)),
        None,
    ];

    // Act
    let mut results = Vec::new();
    for (index, email) in emails.into_iter().enumerate() {
        let subject = format!("refused-{index}");
        results.push(
            sign_in(&rig.world, OIDC, &subject, Some(INVITE), |grant| {
                grant.email = email.map(|(address, verified)| (address.to_owned(), verified));
            })
            .await,
        );
    }

    // Assert
    for result in &results {
        assert!(invitation_required(result), "{result:?}");
    }
    let counts = rig.world.counts().await;
    assert_eq!((counts.accounts, counts.identities), (0, 0));
    assert_eq!(rig.redemption_of(invite).await, (None, false));
    rig.finish().await;
}

#[tokio::test]
async fn with_an_allowlist_a_listed_address_is_matched_without_regard_to_case_and_stored_as_verified()
 {
    // Arrange
    let mut rig = allowlisted().await;
    rig.member_invite(INVITE).await;
    rig.world.idp.behave(|behaviour| {
        behaviour.github_emails = vec![("Victim@Example.TEST".to_owned(), true, true)];
    });

    // Act
    let result = sign_in(&rig.world, GITHUB, "5150", Some(INVITE), |_| {}).await;

    // Assert
    assert!(
        matches!(result, Ok(SignInOutcome::Session { .. })),
        "{result:?}"
    );
    let stored: (Option<String>, bool) = sqlx::query_as(
        "SELECT email, email_verified FROM identities WHERE provider_user_id = '5150'",
    )
    .fetch_one(&mut rig.world.conn)
    .await
    .expect("read the identity");
    assert_eq!(stored, (Some("Victim@Example.TEST".to_owned()), true));
    assert_eq!(rig.world.idp.hits("/user/emails"), 1);
    rig.finish().await;
}

#[tokio::test]
async fn without_an_allowlist_no_email_is_read_or_stored() {
    // Arrange
    let mut rig = Rig::new(POOL, |config| config.email_allowlist.clear(), Vec::new()).await;
    rig.member_invite("sc_inv_i-github").await;
    rig.member_invite("sc_inv_i-oidc").await;
    rig.world.idp.behave(|behaviour| {
        behaviour.github_emails = vec![(VICTIM.to_owned(), true, true)];
    });

    // Act
    let github = sign_in(&rig.world, GITHUB, "4040", Some("sc_inv_i-github"), |_| {}).await;
    let oidc = sign_in(&rig.world, OIDC, "plain", Some("sc_inv_i-oidc"), |grant| {
        grant.email = Some((VICTIM.to_owned(), true));
    })
    .await;

    // Assert
    assert!(
        matches!(github, Ok(SignInOutcome::Session { .. })),
        "{github:?}"
    );
    assert!(
        matches!(oidc, Ok(SignInOutcome::Session { .. })),
        "{oidc:?}"
    );
    assert_eq!(rig.world.idp.hits("/user/emails"), 0);
    let stored: Vec<(Option<String>, bool)> =
        sqlx::query_as("SELECT email, email_verified FROM identities")
            .fetch_all(&mut rig.world.conn)
            .await
            .expect("read the identities");
    assert_eq!(stored, [(None, false), (None, false)]);
    rig.finish().await;
}
