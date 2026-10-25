//! Typed procedures shared by in-process calls, HTTP and schema generation.
//!
//! The Rust port is an initial vertical slice. The retained OCaml source is
//! the reference for behavior that has not yet been implemented here.

pub use loams_web_core::*;

#[cfg(feature = "http")]
pub use loams_web_http as http;

#[cfg(feature = "openapi")]
pub use loams_web_openapi as openapi;
