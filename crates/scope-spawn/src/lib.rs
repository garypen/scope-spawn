//!
//! A Small utility library which aims to make structured concurrency a
//! bit easier when used with tokio or tower.
//!
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
