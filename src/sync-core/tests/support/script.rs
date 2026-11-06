//! Ordered answers per route.

use std::collections::{HashMap, VecDeque};

use bytes::Bytes;

use super::answer::Answer;
use super::route::Route;

#[derive(Default)]
struct RouteScript {
    queue: VecDeque<Answer>,
    repeat: Option<Answer>,
}

/// What the fixture answers per route: each request takes the next queued answer; when the queue is
/// empty the `always` answer repeats; with neither the request gets the "unscripted" problem.
#[derive(Default)]
pub struct Script {
    routes: HashMap<Route, RouteScript>,
}

impl Script {
    pub fn new() -> Self {
        Script::default()
    }

    pub fn on(mut self, route: Route, answer: Answer) -> Self {
        self.routes
            .entry(route)
            .or_default()
            .queue
            .push_back(answer);
        self
    }

    pub fn on_times(mut self, route: Route, answer: Answer, times: usize) -> Self {
        let entry = self.routes.entry(route).or_default();
        entry.queue.extend(std::iter::repeat_n(answer, times));
        self
    }

    pub fn always(mut self, route: Route, answer: Answer) -> Self {
        self.routes.entry(route).or_default().repeat = Some(answer);
        self
    }

    pub(super) fn next(&mut self, route: Route) -> Option<Answer> {
        let entry = self.routes.get_mut(&route)?;
        entry.queue.pop_front().or_else(|| entry.repeat.clone())
    }
}

/// `len` bytes from `seed`: the same seed gives the same bytes, so a script built twice sends the same content.
pub fn seeded_bytes(seed: u64, len: usize) -> Bytes {
    let mut state = seed;
    let mut out = Vec::with_capacity(len + 8);
    while out.len() < len {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        out.extend_from_slice(&(z ^ (z >> 31)).to_le_bytes());
    }
    out.truncate(len);
    Bytes::from(out)
}
