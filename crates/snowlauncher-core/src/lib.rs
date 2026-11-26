//! Domain types and rules for Snow Launcher.
//!
//! This crate performs no direct IO. Anything that touches the outside
//! world is expressed as a trait here and implemented in another crate.

pub mod network;
pub mod process;
pub mod sleep;
