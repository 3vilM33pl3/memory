extern crate self as mem_service;

#[allow(dead_code)]
#[path = "main.rs"]
mod runtime;

mod repository;

pub use repository::*;
pub use runtime::run_service;
