//!
//! A Small utility library which aims to make structured concurrency a
//! bit easier when used with tokio or tower.
//!
//! A Simple Example
//!
//! ```rust
//! use scope_spawn::scope::Scope;
//!
//! #[tokio::main]
//! async fn main() {
//!     let scope = Scope::new();
//!     scope.spawn(async {
//!     println!("Hello from a spawned task!");
//! });
//! // scope is dropped here, and spawned tasks are cancelled.
//! }
//! ```
#![warn(
    missing_docs,
    missing_debug_implementations,
    missing_copy_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    unused_qualifications
)]

pub mod scope;
