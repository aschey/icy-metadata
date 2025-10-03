#![deny(missing_docs)]
#![forbid(unsafe_code)]
#![forbid(clippy::unwrap_used)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

pub mod error;
mod headers;
mod parse;
mod reader;

pub use headers::*;
pub use reader::*;
