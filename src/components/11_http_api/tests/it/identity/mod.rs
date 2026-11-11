//! The identity, account and admin handlers of 11 (E-21/c12): sign-in, sessions, devices, the own account and the
//! admin routes over the whole server on real TCP, with the ports of 01 and 07 as recording stubs and 01's
//! authenticator as a double.

mod account;
mod admin;
mod devices;
mod limits;
mod operations;
mod redaction;
mod registry;
mod sessions;
mod signin;
mod support;
