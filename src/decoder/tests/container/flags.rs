//! AC-1: the flags of a running decoder container are the production flags of `deploy/compose/compose.dev.yml`.

use std::fs;

use serde_json::Value;

use crate::docker::docker_ok;
use crate::host::seccomp_profile;

pub const PIDS_LIMIT: u64 = 512;

/// Inspects the container and panics, naming every flag that differs, so that no case runs on a container
/// that is not the one that ships.
pub fn assert_production_flags(name: &str, label: &str, cpus: u64, memory_bytes: u64) {
    let inspected: Value =
        serde_json::from_str(&docker_ok(&["inspect", name])).expect("inspect json");
    let (host, config) = (&inspected[0]["HostConfig"], &inspected[0]["Config"]);
    let mut problems = Vec::new();
    let mut want = |what: &str, ok: bool| {
        if !ok {
            problems.push(what.to_owned());
        }
    };
    want("network_mode none", host["NetworkMode"] == "none");
    want("read-only root filesystem", host["ReadonlyRootfs"] == true);
    want(
        "cap_drop ALL",
        host["CapDrop"] == serde_json::json!(["ALL"]),
    );
    want(
        "no cap_add",
        host["CapAdd"].as_array().is_none_or(Vec::is_empty),
    );
    want("not privileged", host["Privileged"] == false);
    want("init", host["Init"] == true);
    want(
        "restart unless-stopped",
        host["RestartPolicy"]["Name"] == "unless-stopped",
    );
    want("pids_limit 512", host["PidsLimit"] == PIDS_LIMIT);
    want("cpus", host["NanoCpus"] == cpus * 1_000_000_000);
    want("memory", host["Memory"] == memory_bytes);
    want("memory plus swap", host["MemorySwap"] == memory_bytes);
    want("user 10003:10003", config["User"] == "10003:10003");
    let options: Vec<&str> = host["SecurityOpt"]
        .as_array()
        .map(|all| all.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    want(
        "no-new-privileges",
        options.contains(&"no-new-privileges:true"),
    );
    let profile: Option<Value> = options
        .iter()
        .find_map(|option| option.strip_prefix("seccomp="))
        .and_then(|text| serde_json::from_str(text).ok());
    let expected: Value = serde_json::from_str(
        &fs::read_to_string(seccomp_profile()).expect("read the seccomp profile"),
    )
    .expect("the seccomp profile is json");
    want(
        "the decoder seccomp profile",
        profile.as_ref() == Some(&expected),
    );
    assert!(
        problems.is_empty(),
        "AC-1: decoder-{} ({}) lacks the production flags: {problems:?}",
        label,
        name
    );
}
