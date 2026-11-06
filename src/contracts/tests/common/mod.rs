#![allow(dead_code)]

use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex, PoisonError};

use contracts::{Fault, Principal, PrincipalKind, Role};
use sc_clock::Timestamp;
use uuid::Uuid;

pub const NOW: Timestamp = Timestamp::from_unix_micros(1_793_956_272_123_456);
pub const IP: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// The method names a double was called with, shared between the double and the test.
#[derive(Clone, Default)]
pub struct Calls(Arc<Mutex<Vec<&'static str>>>);

impl Calls {
    pub fn record(&self, method: &'static str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(method);
    }

    pub fn sorted(&self) -> Vec<&'static str> {
        let mut calls = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        calls.sort_unstable();
        calls
    }
}

pub fn sorted(mut names: Vec<&'static str>) -> Vec<&'static str> {
    names.sort_unstable();
    names
}

pub fn id() -> Uuid {
    Uuid::from_u128(7)
}

pub fn principal() -> Principal {
    Principal {
        account_id: id(),
        kind: PrincipalKind::Session,
        session_or_device_id: id(),
        role: Role::User,
        label: "test".to_owned(),
    }
}

pub fn fault() -> Fault {
    Box::new(io::Error::other("planted fault"))
}
