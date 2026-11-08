//! The blobstore doctor checks: key (AC-6), volume, retention and missing blobs (AC-4).

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use contracts::{DoctorCheck, DoctorContext, DoctorTag, FindingLevel};
use sunnycloud_blobstore::{KeyDoctorCheck, MissingBlobsCheck, RetentionCheck, VolumeCheck};

use crate::fixture::{KEY_TWO, Rotated};
use crate::inventory::{inventory, volume_with_gaps};
use crate::support::{KEY, Rig};

const DAY: Duration = Duration::from_secs(86_400);

fn levels(findings: &[contracts::Finding]) -> Vec<FindingLevel> {
    findings.iter().map(|finding| finding.level).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_missing_blob_check_lists_the_same_seven_ids_as_fail_findings() {
    // Arrange
    let rig = Rig::new().await;
    let volume = volume_with_gaps(&rig).await;
    let check = MissingBlobsCheck::new(Arc::new(inventory(&rig)), 20);

    // Act
    let findings = check.run(&DoctorContext::default()).await;

    // Assert
    assert_eq!(findings.len(), 7);
    assert!(
        levels(&findings)
            .iter()
            .all(|level| *level == FindingLevel::Fail)
    );
    for id in &volume.missing {
        let named = findings
            .iter()
            .filter(|finding| finding.message.contains(&id.to_string()))
            .count();
        assert_eq!(named, 1, "blob {id}");
    }
    for id in volume.uploading.iter().chain(&volume.present) {
        assert!(
            findings
                .iter()
                .all(|f| !f.message.contains(&id.to_string()))
        );
    }
    assert_eq!(check.tags(), &[DoctorTag::Restore]);
    drop(check);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_missing_blob_check_is_one_ok_finding_when_every_file_is_there() {
    // Arrange
    let rig = Rig::new().await;
    rig.store(1024, 1).await;
    let check = MissingBlobsCheck::new(Arc::new(inventory(&rig)), 20);

    // Act
    let findings = check.run(&DoctorContext::default()).await;

    // Assert
    assert_eq!(levels(&findings), vec![FindingLevel::Ok]);
    drop(check);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_missing_blob_check_fails_once_when_the_walk_cannot_finish() {
    // Arrange
    let rig = Rig::new().await;
    volume_with_gaps(&rig).await;
    let check = MissingBlobsCheck::new(Arc::new(inventory(&rig)), 20);
    rig.db
        .admin_exec("ALTER TABLE blobs RENAME TO blobs_hidden")
        .await;

    // Act
    let findings = check.run(&DoctorContext::default()).await;

    // Assert
    assert_eq!(levels(&findings), vec![FindingLevel::Fail]);
    drop(check);
    rig.finish().await;
}

fn key_check(rig: &Rig, config: sunnycloud_blobstore::BlobstoreConfig) -> KeyDoctorCheck {
    KeyDoctorCheck::new(config, rig.api.clone())
}

fn config_with_key(rig: &Rig, name: &str, bytes: &[u8]) -> sunnycloud_blobstore::BlobstoreConfig {
    let file = rig._scratch.0.join(name);
    fs::write(&file, bytes).expect("key file");
    let mut config = rig.config.clone();
    config.master_key_file = sc_config::Secret::new(file.to_string_lossy().into_owned());
    config
}

#[tokio::test(flavor = "multi_thread")]
async fn the_installed_key_passes_and_nothing_is_written() {
    // Arrange
    let rig = Rig::new().await;
    let check = key_check(&rig, rig.config.clone());
    let before = fs::read(rig.volume.join(".keycheck")).expect("key check");

    // Act
    let findings = check.run(&DoctorContext::default()).await;

    // Assert
    assert_eq!(levels(&findings), vec![FindingLevel::Ok]);
    assert_eq!(
        fs::read(rig.volume.join(".keycheck")).expect("key check"),
        before
    );
    assert_eq!(check.tags(), &[DoctorTag::PreFlight, DoctorTag::Restore]);
    drop(check);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_different_master_key_is_a_fail_finding_without_key_bytes() {
    // Arrange
    let rig = Rig::new().await;
    let check = key_check(&rig, config_with_key(&rig, "other.key", &KEY_TWO));

    // Act
    let findings = check.run(&DoctorContext::default()).await;

    // Assert
    assert_eq!(levels(&findings), vec![FindingLevel::Fail]);
    let text = format!("{} {}", findings[0].message, findings[0].remedy);
    assert!(
        text.contains("data is unreadable without the master key"),
        "{text}"
    );
    for secret in [KEY_TWO, KEY] {
        let hex: String = secret.iter().map(|byte| format!("{byte:02x}")).collect();
        assert!(!text.contains(&hex));
        assert!(!text.contains(&String::from_utf8_lossy(&secret).into_owned()));
    }
    drop(check);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_missing_or_short_key_file_is_a_fail_finding() {
    // Arrange
    let rig = Rig::new().await;
    let mut gone = rig.config.clone();
    gone.master_key_file = sc_config::Secret::new("/nonexistent/master.key".to_owned());
    let missing = key_check(&rig, gone);
    let short = key_check(&rig, config_with_key(&rig, "short.key", &[7; 31]));

    // Act
    let missing_findings = missing.run(&DoctorContext::default()).await;
    let short_findings = short.run(&DoctorContext::default()).await;

    // Assert
    assert_eq!(levels(&missing_findings), vec![FindingLevel::Fail]);
    assert_eq!(levels(&short_findings), vec![FindingLevel::Fail]);
    drop((missing, short));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_the_check_does_not_know_yet_is_accepted_without_recording_it() {
    // Arrange
    let rig = Rig::new().await;
    let rotated = Rotated::start(&rig).await;
    let check = key_check(&rig, rotated.config.clone());
    let before = fs::read(rig.volume.join(".keycheck")).expect("key check");

    // Act
    let findings = check.run(&DoctorContext::default()).await;

    // Assert
    assert_eq!(levels(&findings), vec![FindingLevel::Ok]);
    assert_eq!(
        fs::read(rig.volume.join(".keycheck")).expect("key check"),
        before,
        "doctor writes nothing"
    );
    drop((check, rotated));
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_key_check_with_blobs_present_fails_unless_it_is_a_pre_flight() {
    // Arrange
    let rig = Rig::new().await;
    rig.store(1024, 1).await;
    fs::remove_file(rig.volume.join(".keycheck")).expect("remove the key check");
    let check = key_check(&rig, rig.config.clone());
    let pre_flight = DoctorContext {
        pre_flight: true,
        ..DoctorContext::default()
    };

    // Act
    let full = check.run(&DoctorContext::default()).await;
    let early = check.run(&pre_flight).await;

    // Assert
    assert_eq!(levels(&full), vec![FindingLevel::Fail]);
    assert_eq!(levels(&early), vec![FindingLevel::Ok]);
    drop(check);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_install_without_a_key_check_passes() {
    // Arrange
    let rig = Rig::new().await;
    fs::remove_file(rig.volume.join(".keycheck")).expect("remove the key check");
    let check = key_check(&rig, rig.config.clone());

    // Act
    let findings = check.run(&DoctorContext::default()).await;

    // Assert
    assert_eq!(levels(&findings), vec![FindingLevel::Ok]);
    drop(check);
    rig.finish().await;
}

fn expects_warning(rig: &Rig) -> bool {
    const XFS: i64 = 0x5846_5342;
    const EXT: i64 = 0xEF53;
    let magic = rustix::fs::statfs(&rig.volume).expect("statfs").f_type as i64;
    magic != XFS && magic != EXT
}

#[tokio::test(flavor = "multi_thread")]
async fn the_volume_check_reports_writable_filesystem_and_space() {
    // Arrange
    let rig = Rig::new().await;
    let check = VolumeCheck::new(&rig.config);
    let warns = expects_warning(&rig);

    // Act
    let findings = check.run(&DoctorContext::default()).await;

    // Assert
    assert_eq!(findings.len(), 3);
    assert_eq!(findings[0].level, FindingLevel::Ok);
    assert_eq!(
        findings[1].level,
        if warns {
            FindingLevel::Warn
        } else {
            FindingLevel::Ok
        }
    );
    assert_eq!(findings[2].level, FindingLevel::Ok);
    assert_eq!(check.tags(), &[DoctorTag::PreFlight]);
    rig.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn free_space_under_the_floor_a_read_only_root_and_a_missing_root_are_fail_findings() {
    // Arrange
    use std::os::unix::fs::PermissionsExt;
    let rig = Rig::new().await;
    let stat = rustix::fs::statvfs(&rig.volume).expect("statvfs");
    let mut high = rig.config.clone();
    high.blob_free_reserve = stat.f_bavail * stat.f_frsize + (64 << 20);
    let mut absent = rig.config.clone();
    absent.blob_root = Some(
        rig.volume
            .join("not-mounted")
            .to_string_lossy()
            .into_owned(),
    );

    // Act
    let low_space = VolumeCheck::new(&high).run(&DoctorContext::default()).await;
    fs::set_permissions(&rig.volume, fs::Permissions::from_mode(0o555)).expect("read-only root");
    let read_only = VolumeCheck::new(&rig.config)
        .run(&DoctorContext::default())
        .await;
    fs::set_permissions(&rig.volume, fs::Permissions::from_mode(0o755)).expect("restore");
    let missing = VolumeCheck::new(&absent)
        .run(&DoctorContext::default())
        .await;

    // Assert
    assert_eq!(low_space[2].level, FindingLevel::Fail);
    assert_eq!(read_only[0].level, FindingLevel::Fail);
    assert_eq!(levels(&missing), vec![FindingLevel::Fail]);
    for finding in low_space.iter().chain(&read_only).chain(&missing) {
        assert!(!finding.message.contains(&*rig.volume.to_string_lossy()));
    }
    rig.finish().await;
}

async fn retention_findings(
    retention: Duration,
    interval: Option<Duration>,
    rig: &Rig,
) -> Vec<FindingLevel> {
    let mut config = rig.config.clone();
    config.retention = retention;
    let ctx = DoctorContext {
        backup_interval: interval,
        ..DoctorContext::default()
    };
    let check = RetentionCheck::new(&config);
    levels(&check.run(&ctx).await)
}

#[tokio::test(flavor = "multi_thread")]
async fn retention_warns_when_it_is_zero_or_shorter_than_the_backup_interval() {
    // Arrange
    let rig = Rig::new().await;
    let week = 7 * DAY;

    // Act
    let plain = retention_findings(week, None, &rig).await;
    let daily = retention_findings(week, Some(DAY), &rig).await;
    let equal = retention_findings(week, Some(week), &rig).await;
    let short = retention_findings(DAY, Some(week), &rig).await;
    let zero = retention_findings(Duration::ZERO, None, &rig).await;
    let zero_and_interval = retention_findings(Duration::ZERO, Some(DAY), &rig).await;

    // Assert
    assert_eq!(plain, vec![FindingLevel::Ok]);
    assert_eq!(daily, vec![FindingLevel::Ok]);
    assert_eq!(equal, vec![FindingLevel::Ok]);
    assert_eq!(short, vec![FindingLevel::Warn]);
    assert_eq!(zero, vec![FindingLevel::Warn]);
    assert_eq!(
        zero_and_interval,
        vec![FindingLevel::Warn, FindingLevel::Warn]
    );
    assert!(RetentionCheck::new(&rig.config).tags().is_empty());
    rig.finish().await;
}
