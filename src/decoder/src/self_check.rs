//! The self-check (10 section 4): one item per isolation property of the running container, as data.
//! A property that cannot be read is reported `ok = false`, never as a missing item, and the call
//! itself never fails: the supervisor decides what a failed item means (09 section 3).

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStrExt;

use media_shared::decoder_protocol::{SelfCheckItem, SelfCheckReport};
use sc_observability::log_error;

mod network;

/// Mount points under these paths mean the blob store or a secret is visible to the decoder
/// (deployment docs; no config key names them).
const FORBIDDEN_MOUNTS: [&str; 2] = ["/var/lib/sunnycloud/blobs", "/run/secrets"];
/// The names a decoder container's environment may hold: the image's own (`PATH`,
/// `VIPS_BLOCK_UNTRUSTED`), the container runtime's (`HOSTNAME`, `HOME`) and the config keys.
const ENVIRONMENT_NAMES: [&str; 4] = ["PATH", "HOSTNAME", "HOME", "VIPS_BLOCK_UNTRUSTED"];
const CONFIG_PREFIX: &str = "SUNNYCLOUD_";

pub fn report(forbidden_targets: &[String]) -> SelfCheckReport {
    let status = read("/proc/self/status");
    let mountinfo = read("/proc/self/mountinfo");
    SelfCheckReport(vec![
        item(
            "forbidden_targets_unreachable",
            network::all_unreachable(forbidden_targets),
        ),
        item(
            "environment_is_allowlist",
            environment_is_allowlist(std::env::vars_os().map(|(name, _)| name)),
        ),
        item(
            "no_blob_or_secret_mounts",
            mountinfo.as_deref().is_some_and(no_forbidden_mounts),
        ),
        item(
            "not_root",
            status.as_deref().and_then(|s| field(s, "Uid")) > Some(0),
        ),
        item(
            "no_capabilities",
            status.as_deref().is_some_and(no_capabilities),
        ),
        item(
            "no_new_privs",
            status.as_deref().and_then(|s| field(s, "NoNewPrivs")) == Some(1),
        ),
        item(
            "root_filesystem_read_only",
            mountinfo.as_deref().is_some_and(root_is_read_only),
        ),
    ])
}

fn item(name: &str, ok: bool) -> SelfCheckItem {
    SelfCheckItem {
        name: name.into(),
        ok,
    }
}

fn read(path: &str) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) => {
            log_error!("self-check cannot read a /proc file", err = &error);
            None
        }
    }
}

/// The first number after `name:` (the real uid for `Uid`).
fn field(status: &str, name: &str) -> Option<u64> {
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))?;
    line.split_whitespace().next()?.parse().ok()
}

/// A capability set as the hexadecimal mask of `CapEff:`, `CapPrm:` or `CapBnd:`.
fn capability_set(status: &str, name: &str) -> Option<u64> {
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))?;
    u64::from_str_radix(line.trim(), 16).ok()
}

/// The effective, permitted and bounding sets are all empty. The effective set alone cannot show a
/// capability the container added: a process that is not root starts with it empty, and only the
/// bounding set keeps the added capability on record.
fn no_capabilities(status: &str) -> bool {
    ["CapEff", "CapPrm", "CapBnd"]
        .iter()
        .all(|name| capability_set(status, name) == Some(0))
}

/// Read from the process, not from `/proc/self/environ`: the runner is not dumpable (no core dump of
/// a decoded file), and that makes its own `environ` unreadable to it.
fn environment_is_allowlist(names: impl IntoIterator<Item = OsString>) -> bool {
    names.into_iter().all(|name| {
        let name = name.as_bytes();
        ENVIRONMENT_NAMES
            .iter()
            .any(|allowed| allowed.as_bytes() == name)
            || name.starts_with(CONFIG_PREFIX.as_bytes())
    })
}

/// `(mount point, mount options)` of each line of a mount table.
fn mounts(mountinfo: &str) -> impl Iterator<Item = (&str, &str)> {
    mountinfo.lines().filter_map(|line| {
        let mut fields = line.split_whitespace().skip(4);
        Some((fields.next()?, fields.next()?))
    })
}

fn no_forbidden_mounts(mountinfo: &str) -> bool {
    mounts(mountinfo).all(|(point, _)| {
        FORBIDDEN_MOUNTS
            .iter()
            .all(|forbidden| point != *forbidden && !point.starts_with(&format!("{forbidden}/")))
    })
}

/// The topmost mount on `/` is the one a process sees.
fn root_is_read_only(mountinfo: &str) -> bool {
    mounts(mountinfo)
        .filter(|(point, _)| *point == "/")
        .last()
        .is_some_and(|(_, options)| options.split(',').any(|option| option == "ro"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "Name:\tx\nUid:\t10003\t10003\t10003\t10003\nCapPrm:\t0000000000000000\n\
        CapEff:\t0000000000000000\nCapBnd:\t0000000000000000\nNoNewPrivs:\t1\n";
    const MOUNTINFO: &str = "\
1582 1486 0:102 / / ro,relatime - overlay overlay rw,lowerdir=/a
1584 1582 0:255 / /proc rw,nosuid,nodev,noexec,relatime - proc proc rw
1595 1582 0:99 / /scratch rw,relatime - ext4 /dev/x rw
";

    #[test]
    fn a_process_without_privileges_passes_the_three_status_items() {
        assert_eq!(
            (
                field(STATUS, "Uid"),
                no_capabilities(STATUS),
                field(STATUS, "NoNewPrivs")
            ),
            (Some(10003), true, Some(1))
        );
    }

    #[test]
    fn root_and_a_missing_no_new_privs_are_read_as_such() {
        let root = "Uid:\t0\t0\t0\t0\nNoNewPrivs:\t1\n";
        let privileged = "Uid:\t10003\t0\t0\t0\nNoNewPrivs:\t0\n";

        assert_eq!(
            (field(root, "Uid"), field(privileged, "NoNewPrivs")),
            (Some(0), Some(0))
        );
    }

    #[test]
    fn a_capability_in_any_one_of_the_three_sets_fails_the_item() {
        let with = |set: &str| {
            STATUS.replace(
                &format!("{set}:\t0000000000000000"),
                &format!("{set}:\t0000000000002000"),
            )
        };

        assert_eq!(
            ["CapEff", "CapPrm", "CapBnd"].map(|set| no_capabilities(&with(set))),
            [false, false, false]
        );
    }

    #[test]
    fn a_status_without_the_capability_lines_fails_the_item() {
        assert!(!no_capabilities("Uid:\t10003\t10003\t10003\t10003\n"));
    }

    #[test]
    fn the_image_and_runtime_environment_and_the_config_keys_are_the_allowlist() {
        let names = [
            "PATH",
            "HOSTNAME",
            "HOME",
            "VIPS_BLOCK_UNTRUSTED",
            "SUNNYCLOUD_DECODER_GID",
        ];

        assert!(environment_is_allowlist(names.map(OsString::from)));
    }

    #[test]
    fn one_extra_variable_leaves_the_allowlist() {
        assert!(!environment_is_allowlist(
            ["PATH", "X_TEST"].map(OsString::from)
        ));
    }

    #[test]
    fn a_name_that_only_starts_like_an_allowed_one_is_extra() {
        assert!(!environment_is_allowlist([OsString::from("PATHS")]));
        assert!(!environment_is_allowlist([OsString::from("SUNNYCLOUD")]));
    }

    #[test]
    fn the_blob_path_and_anything_under_secrets_is_a_forbidden_mount() {
        let blobs =
            format!("{MOUNTINFO}1600 1582 8:1 / /var/lib/sunnycloud/blobs rw - ext4 /dev/y rw\n");
        let secret = format!("{MOUNTINFO}1601 1582 8:1 / /run/secrets/db_url ro - tmpfs t ro\n");

        assert_eq!(
            (
                no_forbidden_mounts(MOUNTINFO),
                no_forbidden_mounts(&blobs),
                no_forbidden_mounts(&secret)
            ),
            (true, false, false)
        );
    }

    #[test]
    fn a_neighbour_of_a_forbidden_path_is_not_a_forbidden_mount() {
        let table = format!("{MOUNTINFO}1600 1582 8:1 / /run/secrets-other rw - tmpfs t rw\n");

        assert!(no_forbidden_mounts(&table));
    }

    #[test]
    fn the_root_is_read_only_when_its_topmost_mount_says_ro() {
        let writable = MOUNTINFO.replace("/ / ro,relatime", "/ / rw,relatime");
        let remounted =
            format!("{MOUNTINFO}1700 1582 0:102 / / rw,relatime - overlay overlay rw\n");

        assert_eq!(
            (
                root_is_read_only(MOUNTINFO),
                root_is_read_only(&writable),
                root_is_read_only(&remounted),
                root_is_read_only("")
            ),
            (true, false, false, false)
        );
    }

    #[test]
    fn the_report_names_the_same_seven_items_on_every_run() {
        let names = |report: SelfCheckReport| -> Vec<String> {
            report.0.into_iter().map(|item| item.name).collect()
        };

        let first = names(report(&[]));
        let second = names(report(&["nowhere.invalid:1".to_owned()]));

        assert_eq!(first, second);
        assert_eq!(first.len(), 7);
    }
}
