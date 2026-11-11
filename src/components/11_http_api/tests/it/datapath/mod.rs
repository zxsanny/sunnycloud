//! The sync data path of 11 (E-21/c13, IT-11-2 and the ordering part of NFT-RES-35): tree, transfer and feed
//! handlers over the whole server on real TCP, with the ports of 02, 03 and 05 as recording stubs and 01's
//! authenticator as a double.

mod commits;
mod content;
mod feed;
mod ordering;
mod registry;
mod streaming;
mod support;
mod tree;
mod uploads;
