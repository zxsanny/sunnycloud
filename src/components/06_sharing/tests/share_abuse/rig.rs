//! One account with a live file, the links the abuse tests need (made by the real `create`), unknown tokens, the
//! visitor addresses and the probe of the `abuse_trip` rows.

use std::net::{IpAddr, Ipv4Addr};

use contracts::{CreatedShare, ShareService};
use sc_ids::RawToken;
use sqlx::{Connection, PgConnection};
use sunnycloud_sharing::{PgShareService, SHARE_TOKEN_PREFIX};
use uuid::Uuid;

use crate::harness::Harness;
use crate::owner_support::account_with_root;
use crate::visitor_support::{named_file, presented};

pub const IP1: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));
pub const IP2: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 2));

/// One `abuse_trip` row: subject type, subject id, address text, detail.
pub type TripRow = (Option<String>, Option<Uuid>, Option<String>, Option<String>);

pub struct Rig {
    pub harness: Harness,
    pub shares: PgShareService,
    account: Uuid,
    file: Uuid,
    probe: PgConnection,
}

impl Rig {
    pub async fn new() -> Rig {
        let harness = Harness::new().await;
        let mut probe = harness.fixture_conn().await;
        let account = Uuid::new_v4();
        let root = account_with_root(&mut probe, account).await;
        let file = named_file(&mut probe, account, root, "f.jpg", 10, "image/jpeg").await;
        let shares = harness.shares();
        Rig {
            harness,
            shares,
            account,
            file,
            probe,
        }
    }

    /// A live link to the file, with `password` when given.
    pub async fn link(&self, password: Option<&str>) -> CreatedShare {
        self.shares
            .create(self.account, self.file, None, password)
            .await
            .expect("create link")
    }

    /// The `abuse_trip` rows, oldest first.
    pub async fn trips(&mut self) -> Vec<TripRow> {
        sqlx::query_as(
            "SELECT subject_type, subject_id, host(ip), detail::text FROM audit_log \
             WHERE event = 'abuse_trip' ORDER BY id",
        )
        .fetch_all(&mut self.probe)
        .await
        .expect("read abuse_trip rows")
    }

    pub async fn finish(self) {
        let Rig {
            harness,
            shares,
            probe,
            ..
        } = self;
        probe.close().await.expect("close probe connection");
        drop(shares);
        harness.finish().await;
    }
}

/// The `n`th well-formed share token that no link has.
pub fn unknown(n: usize) -> RawToken {
    presented(format!("{SHARE_TOKEN_PREFIX}{n:0>43}"))
}

/// The `n`th address of a block that no other test uses.
pub fn address(n: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(198, 51, 100, n))
}
