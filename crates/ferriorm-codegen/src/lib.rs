#![warn(clippy::pedantic)]

//! Code generator that produces a type-safe Rust client from a validated schema.
//!
//! Given an [`ferriorm_core::schema::Schema`], this crate emits a complete Rust
//! module tree consisting of:
//!
//! - **Model structs** with `sqlx::FromRow` derives ([`model`])
//! - **Enum definitions** with `sqlx::Type` derives ([`enums`])
//! - **Filter / order / data submodules** for type-safe queries ([`model`])
//! - **Relation types** and batched include loading ([`relations`])
//! - **`FerriormClient`** -- the entry-point struct users interact with ([`client`])
//!
//! The main entry point is [`generator::generate`], which writes all files to
//! the configured output directory.
//!
//! # Related crates
//!
//! - `ferriorm_core` -- the `Schema` IR consumed by this crate.
//! - `ferriorm_parser` -- parses `.ferriorm` files into the `Schema` IR.
//! - `ferriorm_runtime` -- the runtime library that generated code depends on.

pub mod client;
pub mod enums;
pub mod formatter;
pub mod generator;
pub mod model;
pub mod relations;

mod rust_type;
