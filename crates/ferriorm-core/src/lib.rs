#![warn(clippy::pedantic)]

//! Core domain types for the ferriorm ecosystem.
//!
//! This crate is the foundation of ferriorm. It defines the Abstract Syntax Tree
//! ([`ast`]), the validated Schema Intermediate Representation ([`schema`]),
//! scalar and provider types ([`types`]), and domain-level errors ([`error`]).
//!
//! `ferriorm-core` has **zero external dependencies** (aside from optional `serde`
//! support behind a feature flag). Every other ferriorm crate depends on it, but it
//! depends on nothing outside `std`.
//!
//! # Crate relationships
//!
//! ```text
//! ferriorm-parser ──┐
//! ferriorm-codegen ─┤
//! ferriorm-runtime ─┼── all depend on ──► ferriorm-core
//! ferriorm-migrate ─┘
//! ```

pub mod ast;
pub mod error;
pub mod schema;
pub mod types;
pub mod utils;
