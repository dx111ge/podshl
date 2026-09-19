//! The part of the client that stands on its own: the record of local fixes,
//! and what it needs to read the machine, change a file and write its log.
//!
//! **Nothing here knows about the protocol.** No operator, no signed log, no
//! agent card, no model and no window. The client builds on this library, and
//! so does `podshl-repairs`, a small program for people who want the record
//! and nothing else. Both read and write the same `repairs.json`, so a record
//! made by one is seen by the other.
//!
//! The rule that keeps it that way is the module list below: a module here may
//! use another module here and nothing else of the client's. The compiler
//! enforces it — anything the client has that this library does not is simply
//! not in scope.

// First, so `m!` is in scope in every module below it.
#[macro_use]
pub mod msg;

pub mod actions;
// What a coding agent changes, recorded through its own hooks.
pub mod agent_hook;
pub mod clientlog;
// What a package declares about itself, taken into the record.
pub mod declared;
pub mod elevate;
pub mod elevated;
pub mod http;
// The client's own question, asked on the person's behalf and requestable by
// no publisher — see the module for why that separation is the point.
pub mod provenance;
pub mod reads;
pub mod redact;
pub mod repair;
pub mod repairs_cli;
pub mod upstream;
