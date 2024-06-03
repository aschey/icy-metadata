#![deny(missing_docs)]
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

/// Header name to request icy metadata.
pub const ICY_METADATA_HEADER: &str = "Icy-MetaData";

/// Appends the `Icy-MetaData` header to the `header_map`.
pub fn add_icy_metadata_header(header_map: &mut HeaderMap) {
    header_map.append(
        ICY_METADATA_HEADER,
        "1".parse().expect("valid header value"),
    );
}

/// Trait for requesting icy metadata from an HTTP request builder
pub trait RequestIcyMetadata {
    /// Appends the `Icy-MetaData` header to the request's header map
    fn request_icy_metadata(self) -> Self;
}

#[cfg(feature = "reqwest")]
impl RequestIcyMetadata for reqwest::ClientBuilder {
    fn request_icy_metadata(self) -> Self {
        let mut header_map = HeaderMap::new();
        add_icy_metadata_header(&mut header_map);
        self.default_headers(header_map)
    }
}

#[cfg(feature = "reqwest")]
impl RequestIcyMetadata for reqwest::RequestBuilder {
    fn request_icy_metadata(self) -> Self {
        let mut header_map = HeaderMap::new();
        add_icy_metadata_header(&mut header_map);
        self.headers(header_map)
    }
}
