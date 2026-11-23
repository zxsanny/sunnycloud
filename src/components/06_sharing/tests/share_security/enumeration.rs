//! ST-06-3 (AZ-3703 AC-3, AC-12, AC-20, NFT-SEC-17, FT-N-09): the 10,000 tokens of the token corpus (random,
//! and near misses of live tokens: one character changed, truncated, case changed) opened from one address. In
//! every minute calls 1 to 100 answer the one `NotFound` and call 101 `RateLimited`; the minute is moved on the
//! manual clock, never slept. No near miss gets an answer a random token does not, and the live links still open.

use std::collections::{BTreeSet, HashMap};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use contracts::{PublicShareError, PublicShareService, ShareOpen};
use sc_ratelimit::LimitId;

use crate::corpora::{Guess, token_corpus};
use crate::harness::Harness;
use crate::link_factory::LinkFactory;
use crate::visitor_support::VISITOR;

const GUESSES: usize = 10_000;
const MINUTE: Duration = Duration::from_secs(60);
const ATTACKER: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 66));

#[tokio::test]
async fn ten_thousand_guesses_get_one_answer_and_trip_the_address_limit_at_call_101() {
    // Arrange
    let harness = Harness::new().await;
    let mut factory = LinkFactory::new(&harness, 5, 4, 0x5106_0003).await;
    let shares = Arc::new(harness.shares());
    let live = factory.links(&shares, 20, 25).await;
    let texts: Vec<String> = live
        .iter()
        .map(|link| link.created.token.expose().to_owned())
        .collect();
    let corpus = token_corpus(&texts, GUESSES, 3);
    let limit = usize::try_from(harness.config.invalid_token_limit).expect("fits");
    assert_eq!(limit, 100, "the AC-20 number");

    // Act
    let mut answers = Vec::with_capacity(GUESSES);
    let mut call_in_minute = 0;
    for (kind, token) in &corpus {
        call_in_minute += 1;
        let answer = shares.open(token, None, ATTACKER).await;
        answers.push((*kind, call_in_minute, answer));
        if call_in_minute == limit + 1 {
            harness.clock.tick(MINUTE);
            call_in_minute = 0;
        }
    }
    let mut live_answers = Vec::new();
    for link in &live {
        live_answers.push(shares.open(&link.created.token, None, VISITOR).await);
    }

    // Assert
    let mut seen: HashMap<(Guess, bool), BTreeSet<String>> = HashMap::new();
    for (kind, call, answer) in &answers {
        let limited = *call > limit;
        if limited {
            let Err(PublicShareError::RateLimited { retry_after }) = answer else {
                panic!("call {call} of a minute is limited: {answer:?}");
            };
            assert!(*retry_after > Duration::ZERO && *retry_after <= MINUTE);
        } else {
            assert!(
                matches!(answer, Ok(ShareOpen::NotFound)),
                "{kind:?} call {call}: {answer:?}"
            );
        }
        seen.entry((*kind, limited))
            .or_default()
            .insert(format!("{answer:?}"));
    }
    for limited in [false, true] {
        let random = &seen[&(Guess::Random, limited)];
        assert_eq!(random.len(), 1, "{random:?}");
        for kind in [Guess::OneCharChanged, Guess::Truncated, Guess::CaseChanged] {
            assert_eq!(&seen[&(kind, limited)], random, "{kind:?}");
        }
    }
    let minutes = u64::try_from(GUESSES / (limit + 1)).expect("fits");
    let counts = harness.limiter.stats().get(LimitId::ShareTokenIp);
    assert_eq!((counts.tripped, counts.limited), (minutes, minutes));
    for (link, answer) in live.iter().zip(&live_answers) {
        let expected_password = link.password.is_some();
        match answer {
            Ok(ShareOpen::PasswordRequired) => assert!(expected_password),
            Ok(ShareOpen::Available {
                name,
                size,
                content_hash,
                ..
            }) => {
                assert!(!expected_password);
                assert_eq!(
                    (name, *size, *content_hash),
                    (&link.file.name, link.file.size, link.file.content_hash)
                );
            }
            other => panic!("a live link stops opening: {other:?}"),
        }
    }
    drop(shares);
    harness.finish().await;
}
