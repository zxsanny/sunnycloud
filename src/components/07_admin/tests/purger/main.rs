//! AZ-3820 (E-17/c06): the deletion purger. `walk` drives the purger over hook doubles and the real 01 claim
//! (the unit rows of the task: order, budget on a manual clock, a failed hook, a held claim); `drain` (IT-07-9),
//! `budget` (IT-07-11) and `overlap` (IT-07-12) run it over the real hooks of 06, 02, 03, 08, 05 and 07a with 04's GC.
//! IT-07-10 and AC-6 read the log lines and live in `purger_logs`; IT-07-13 records statements in `purger_idle`.

#[path = "../purge_support/mod.rs"]
mod purge_support;
#[path = "../support/mod.rs"]
mod support;

mod budget;
mod drain;
mod overlap;
mod walk;
mod walk_support;
