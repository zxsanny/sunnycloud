//! The network part of the self-check: a resolve and a connect to each forbidden target must fail.
//! The attempts run side by side under one deadline short enough for the watchdog, because the
//! self-check runs on the thread that feeds it; a target that has not failed by then counts as
//! reachable, since nothing proved it is not.

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::channel;
use std::thread;
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_millis(1500);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(1000);

pub fn all_unreachable(targets: &[String]) -> bool {
    let (sender, results) = channel();
    for target in targets {
        let (sender, target) = (sender.clone(), target.clone());
        thread::spawn(move || {
            // The receiver is gone when the deadline passed: nothing is left to tell.
            sender.send(reachable(&target)).unwrap_or_default();
        });
    }
    drop(sender);
    let deadline = Instant::now() + DEADLINE;
    for _ in targets {
        match results.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(false) => {}
            Ok(true) | Err(_) => return false,
        }
    }
    true
}

/// True when `host:port` resolves and a connection to one of its addresses opens. A target that is
/// not `host:port` cannot be attempted and so cannot be shown unreachable.
fn reachable(target: &str) -> bool {
    let Ok(addresses) = target.to_socket_addrs() else {
        return target
            .rsplit_once(':')
            .is_none_or(|(_, port)| port.parse::<u16>().is_err());
    };
    addresses
        .into_iter()
        .any(|address| TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn a_listening_port_is_reachable_and_one_listener_fails_the_whole_list() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let open = format!("127.0.0.1:{}", listener.local_addr().expect("addr").port());

        assert!(!all_unreachable(&["nowhere.invalid:1".to_owned(), open]));
    }

    #[test]
    fn a_closed_port_and_a_name_that_does_not_resolve_are_unreachable() {
        let closed = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            format!("127.0.0.1:{}", listener.local_addr().expect("addr").port())
        };

        assert!(all_unreachable(&[closed, "nowhere.invalid:443".to_owned()]));
    }

    #[test]
    fn no_target_is_vacuously_unreachable() {
        assert!(all_unreachable(&[]));
    }

    #[test]
    fn a_target_without_a_port_cannot_be_shown_unreachable() {
        assert!(!all_unreachable(&["api".to_owned()]));
    }
}
