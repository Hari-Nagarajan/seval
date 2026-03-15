//! Session module.
//!
//! Manages session persistence, save/resume functionality, and SEVAL-CLI
//! format compatibility. Backed by `SQLite` via `rusqlite`.

pub mod db;
pub mod import_export;
pub mod memory_tool;
pub mod models;
