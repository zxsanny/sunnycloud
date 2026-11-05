//! AC-7: the export is bounded, complete, filtered and ids only.

mod support;

use std::collections::BTreeSet;
use std::time::Duration;

use sc_audit::{
    AuditDetail, AuditEvent, AuditEventKind, AuditReader, EXPORT_PAGE_ROWS, StoredValue, SubjectRef,
};
use sc_clock::Timestamp;
use support::{Fixture, START, event};
use uuid::Uuid;

const ROWS: usize = 2_500;

/// 2,500 rows: every second is `share_created` (1,250 matches), the rest alternate between two other kinds;
/// the time advances one second per row.
async fn seed(fx: &Fixture) -> Uuid {
    let account = Uuid::new_v4();
    // One statement per row would take long; the rows are written by the writer in a single transaction.
    fx.api
        .run(sc_db::Scope::Account(account), async |tx| {
            for index in 0..ROWS {
                let kind = match index % 4 {
                    0 | 2 => AuditEventKind::ShareCreated,
                    1 => AuditEventKind::SignIn,
                    _ => AuditEventKind::AdminAction,
                };
                fx.clock.tick(Duration::from_secs(1));
                fx.writer
                    .record_in(
                        tx,
                        AuditEvent {
                            actor_account_id: Some(account),
                            subject: (kind == AuditEventKind::ShareCreated).then(|| SubjectRef {
                                subject_type: "share_link",
                                id: Uuid::new_v4(),
                            }),
                            ip: Some("2001:db8::7".parse().expect("ip")),
                            detail: AuditDetail::new()
                                .with_int("seq", i64::try_from(index).expect("small"))
                                .with_code("origin", "web"),
                            ..event(kind)
                        },
                    )
                    .await?;
            }
            Ok::<_, sc_audit::AuditError>(())
        })
        .await
        .expect("seed rows");
    account
}

#[tokio::test]
async fn export_pages_are_bounded_complete_without_repeats_and_filtered() {
    let fx = Fixture::new().await;
    let account = seed(&fx).await;
    let reader = AuditReader::new(fx.api.clone());

    let mut cursor = None;
    let mut pages = 0;
    let mut ids = Vec::new();
    loop {
        let page = reader
            .export(START, Some(AuditEventKind::ShareCreated), cursor)
            .await
            .expect("page");
        assert!(
            page.rows.len() <= EXPORT_PAGE_ROWS,
            "page of {} rows",
            page.rows.len()
        );
        assert!(
            page.rows
                .iter()
                .all(|row| row.event == AuditEventKind::ShareCreated)
        );
        ids.extend(page.rows.iter().map(|row| row.id));
        pages += 1;
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    let unfiltered: i64 = {
        let mut total = 0;
        let mut cursor = None;
        loop {
            let page = reader.export(START, None, cursor).await.expect("page");
            assert!(page.rows.len() <= EXPORT_PAGE_ROWS);
            total += i64::try_from(page.rows.len()).expect("small");
            cursor = page.next;
            if cursor.is_none() {
                break total;
            }
        }
    };

    assert_eq!(ids.len(), ROWS / 2);
    assert_eq!(
        ids.iter().collect::<BTreeSet<_>>().len(),
        ids.len(),
        "each row once"
    );
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]),
        "insertion order"
    );
    assert_eq!(pages, 3, "1,250 matches in pages of 500");
    assert_eq!(unfiltered, i64::try_from(ROWS).expect("small"));
    let matching = fx
        .db
        .admin_count(&format!("SELECT count(*) FROM audit_log WHERE event = 'share_created' AND actor_account_id = '{account}'"))
        .await;
    assert_eq!(matching, i64::try_from(ids.len()).expect("small"));
    fx.db.drop_db().await;
}

#[tokio::test]
async fn export_of_all_rows_takes_five_pages_and_every_row_holds_ids_codes_and_integers_only() {
    let fx = Fixture::new().await;
    seed(&fx).await;
    let reader = AuditReader::new(fx.api.clone());

    let mut cursor = None;
    let mut sizes = Vec::new();
    let mut ids = Vec::new();
    let mut checked = 0;
    loop {
        let page = reader.export(START, None, cursor).await.expect("page");
        sizes.push(page.rows.len());
        for row in &page.rows {
            ids.push(row.id);
            assert_eq!(row.ip, Some("2001:db8::7".parse().expect("ip")));
            assert!(row.actor_account_id.is_some());
            if let Some(subject_type) = &row.subject_type {
                assert_eq!(subject_type, "share_link");
                assert!(row.subject_id.is_some());
            }
            let detail = row.detail.as_ref().expect("detail");
            assert_eq!(detail.len(), 2);
            for (key, value) in detail {
                match (key.as_str(), value) {
                    ("seq", StoredValue::Int(_)) | ("origin", StoredValue::Code(_)) => {}
                    other => panic!("unexpected detail entry {other:?}"),
                }
            }
            checked += 1;
        }
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }

    assert_eq!(sizes, [EXPORT_PAGE_ROWS; 5]);
    assert_eq!(checked, ROWS);
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), ROWS);
    fx.db.drop_db().await;
}

#[tokio::test]
async fn export_since_skips_older_rows_and_an_empty_table_is_one_empty_page() {
    let fx = Fixture::new().await;
    let reader = AuditReader::new(fx.api.clone());
    let empty = reader
        .export(START, None, None)
        .await
        .expect("empty export");
    fx.writer
        .record(event(AuditEventKind::SignIn))
        .await
        .expect("early");
    fx.clock.tick(Duration::from_secs(60));
    fx.writer
        .record(event(AuditEventKind::SignIn))
        .await
        .expect("late");
    let since = Timestamp::from_unix_micros(START.as_unix_micros() + 30_000_000);

    let page = reader.export(since, None, None).await.expect("export");

    assert_eq!((empty.rows.len(), empty.next), (0, None));
    assert_eq!(page.rows.len(), 1);
    assert_eq!(
        page.rows[0].at.as_unix_micros(),
        START.as_unix_micros() + 60_000_000
    );
    assert_eq!(page.next, None);
    fx.db.drop_db().await;
}
