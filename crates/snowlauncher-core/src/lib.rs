//! Domain types and rules for Snow Launcher.
//!
//! This crate performs no direct IO. Anything that touches the outside
//! world is expressed as a trait here and implemented in another crate.

pub mod battlenet_log;
pub mod config;
pub mod desktop;
pub mod errors;
pub mod network;
pub mod process;
pub mod sleep;
pub mod tcp;
