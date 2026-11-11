//! The internal listener, the worker routes and the test-clock endpoints (E-21/c15, IT-11-9 and FT-N-35): the
//! whole server over real TCP, with 01's and 08's authenticators and `WorkerJobs` as recording doubles.

mod clock;
mod listeners;
mod operations;
mod outputs;
mod support;
