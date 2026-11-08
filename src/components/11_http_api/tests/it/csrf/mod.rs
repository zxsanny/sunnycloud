//! The CSRF origin check (E-21/c08, IT-11-6 layer part, AC-58): Mode S over real TCP, one route per method
//! for each user auth class, with the two authentication ports as doubles.

mod bearer;
mod config;
mod cookie;
mod reads;
mod support;
