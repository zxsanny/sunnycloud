//! The registry-driven security matrices (E-21/c18, IT-11-6, IT-11-10, IT-11-11): the CSRF matrix (AC-58) and
//! the two-account matrix (AC-59), generated from `HttpServer::openapi()` and run by one runner on the Mode S
//! server or, for E-21/c20, on a real stack named by its base URL.

mod coverage;
mod csrf;
mod document;
mod gates;
mod gates_identity;
mod idor;
mod plans;
mod run;
mod run_idor;
mod samples;
mod synth;
mod target;
mod world;
