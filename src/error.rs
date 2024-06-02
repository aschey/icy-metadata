use std::error::Error;
use std::fmt::Display;
use std::string::FromUtf8Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataParseError {
    InvalidUtf8(FromUtf8Error),
    Empty(EmptyMetadataError),
}

impl Display for MetadataParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "Failed to parse icy metadata block as a string. The stream may not be properly \
             encoded.",
        )
    }
}

impl Error for MetadataParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyMetadataError(pub String);

impl Display for EmptyMetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "No valid values found for metadata block {}", self.0)
    }
}

impl Error for EmptyMetadataError {}
