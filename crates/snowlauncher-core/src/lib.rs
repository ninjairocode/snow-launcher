//! Domain types and rules for Snow Launcher.
//!
//! This crate performs no direct IO. Anything that touches the outside
//! world is expressed as a trait here and implemented in another crate.

pub mod battlenet_log;
pub mod config;
pub mod dbus;
pub mod desktop;
pub mod display;
pub mod errors;
pub mod house;
pub mod import;
pub mod install;
pub mod installer;
pub mod library;
pub mod network;
pub mod process;
pub mod registry;
pub mod shutdown;
pub mod sleep;
pub mod tcp;
pub mod tray;
