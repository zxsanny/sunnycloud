//! Connection shutdown of `run` (E-21/c17, IT-11-16): the whole server over real TCP while requests are held at
//! `WorkerJobs` and downloads stream through the worker input route. The change-feed long-poll route is not
//! registered yet, so a held `claim` stands in for a held long-poll: like a held read it waits on
//! `ApiServices::feed_shutdown` and is answered when `run` cancels it.

mod downloads;
mod order;
mod support;
