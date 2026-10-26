//! hogs: find what's hogging your disk.
//!
//! `scan` and `fsinfo` are the core (no UI code, fully testable);
//! `app`, `ui` and `theme` are the terminal interface on top.

pub mod app;
pub mod config;
pub mod fsinfo;
pub mod scan;
pub mod theme;
pub mod ui;
pub mod util;
