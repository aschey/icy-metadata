// #![deny(missing_docs)]
#![forbid(unsafe_code)]
#![forbid(clippy::unwrap_used)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(clippy::semicolon_if_nothing_returned)]
#![warn(clippy::doc_markdown)]
#![warn(clippy::default_trait_access)]
#![warn(clippy::ignored_unit_patterns)]
#![warn(clippy::semicolon_if_nothing_returned)]
#![warn(clippy::missing_fields_in_debug)]
#![warn(clippy::use_self)]
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#![doc = include_str!("../README.md")]

pub mod error;
mod headers;
mod parse;
mod reader;

pub use headers::*;
use http::HeaderMap;
pub use reader::*;

pub trait RequestIcyMetadata {
    fn request_icy_metadata(&mut self);
}

impl RequestIcyMetadata for HeaderMap {
    fn request_icy_metadata(&mut self) {
        self.append("Icy-MetaData", "1".parse().expect("valid header value"));
    }
}
