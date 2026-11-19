#![forbid(unsafe_code)]
//! Core contains no GUI. Both CLI and GUI must use these same storage, security and protocol APIs.
pub mod configuration;
pub mod input;
pub mod protocol;
pub mod reconnect;
pub mod secrets;
