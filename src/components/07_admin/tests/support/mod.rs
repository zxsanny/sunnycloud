//! Doubles for the ports 07 consumes, and the settings loader. The skeleton never calls a port, so every
//! double method answers with a typed failure that names the double; a test that reached one would show it.

#![allow(dead_code)]

pub mod doubles;
pub mod settings;
