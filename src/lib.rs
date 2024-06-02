use std::collections::HashMap;
use std::error::Error;
use std::fmt::Display;
use std::io::{self, Read, Seek, SeekFrom};
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::string::FromUtf8Error;

use http::{HeaderMap, HeaderValue};

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct IcyHeaders {
    bitrate: Option<u32>,
    genre: Option<String>,
    stream_name: Option<String>,
    station_url: Option<String>,
    description: Option<String>,
    public: Option<bool>,
    notice1: Option<String>,
    notice2: Option<String>,
    meta_interval: Option<usize>,
    audio_info: Option<IcyAudioInfo>,
}

fn find_header<'a>(search: &[&'a str], headers: &'a HeaderMap) -> Option<&'a HeaderValue> {
    for header in search {
        if let Some(val) = headers.get(*header) {
            return Some(val);
        }
    }
    None
}

impl IcyHeaders {
    pub fn parse_from_headers(headers: &HeaderMap) -> Self {
        // Most header names taken from here https://github.com/xiph/Icecast-Server/blob/master/src/source.c
        Self {
            bitrate: find_header(&["ice-bitrate", "icy-br", "x-audiocast-bitrate"], headers)
                .and_then(|val| val.to_str().ok())
                .and_then(|val| val.split(',').next()?.parse().ok()),
            genre: find_header(&["ice-genre", "icy-genre", "x-audiocast-genre"], headers)
                .and_then(|val| Some(val.to_str().ok()?.to_string())),
            stream_name: find_header(&["ice-name", "icy-name", "x-audiocast-name"], headers)
                .and_then(|val| Some(val.to_str().ok()?.to_string())),
            description: find_header(
                &[
                    "ice-description",
                    "icy-description",
                    "x-audiocast-description",
                ],
                headers,
            )
            .and_then(|val| Some(val.to_str().ok()?.to_string())),
            station_url: find_header(&["ice-url", "icy-url", "x-audiocast-url"], headers)
                .and_then(|val| Some(val.to_str().ok()?.to_string())),
            notice1: find_header(
                &["ice-notice1", "icy-notice1", "x-audiocast-notice1"],
                headers,
            )
            .and_then(|val| Some(val.to_str().ok()?.to_string())),
            notice2: find_header(
                &["ice-notice2", "icy-notice2", "x-audiocast-notice2"],
                headers,
            )
            .and_then(|val| Some(val.to_str().ok()?.to_string())),
            public: find_header(
                &["ice-public", "icy-pub", "icy-public", "x-audiocast-public"],
                headers,
            )
            .and_then(|val| Some(val.to_str().ok()?.to_string()))
            .map(|public| {
                // 1 and 0 are the only supported values, but we'll look for "true" as well
                // because... why not
                public == "1" || public.to_ascii_lowercase() == "true"
            }),
            meta_interval: headers
                .get("icy-metaint")
                .and_then(|val| val.to_str().ok()?.to_string().parse().ok()),
            audio_info: headers.get("ice-audio-info").and_then(|val| {
                let ParseResult { map, .. } = parse_delimited_string(val.to_str().ok()?);
                Some(IcyAudioInfo::parse_from_map(map))
            }),
        }
    }

    pub fn bitrate(&self) -> Option<u32> {
        self.bitrate
    }

    pub fn genre(&self) -> Option<&str> {
        self.genre.as_deref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn stream_name(&self) -> Option<&str> {
        self.stream_name.as_deref()
    }

    pub fn station_url(&self) -> Option<&str> {
        self.station_url.as_deref()
    }

    pub fn notice1(&self) -> Option<&str> {
        self.notice1.as_deref()
    }

    pub fn notice2(&self) -> Option<&str> {
        self.notice2.as_deref()
    }

    pub fn public(&self) -> Option<bool> {
        self.public
    }

    pub fn meta_interval(&self) -> Option<usize> {
        self.meta_interval
    }

    pub fn audio_info(&self) -> Option<&IcyAudioInfo> {
        self.audio_info.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct IcyAudioInfo {
    sample_rate: Option<u32>,
    bitrate: Option<u32>,
    channels: Option<u16>,
    quality: Option<String>,
    custom: HashMap<String, String>,
}

impl IcyAudioInfo {
    fn parse_from_map(map: HashMap<&str, &str>) -> Self {
        let mut info = Self {
            sample_rate: None,
            bitrate: None,
            channels: None,
            quality: None,
            custom: HashMap::new(),
        };
        for (key, value) in map {
            let Ok(key) = urlencoding::decode(key) else {
                continue;
            };
            let Ok(value) = urlencoding::decode(value) else {
                continue;
            };
            match key.to_ascii_lowercase().as_str() {
                "icy-samplerate" | "ice-samplerate" | "samplerate" => {
                    info.sample_rate = value.parse().ok();
                }
                "icy-bitrate" | "ice-bitrate" | "bitrate" => {
                    info.bitrate = value.parse().ok();
                }
                "icy-channels" | "ice-channels" | "channels" => {
                    info.channels = value.parse().ok();
                }
                "icy-quality" | "ice-quality" | "quality" => {
                    info.quality = value.parse().ok();
                }
                _ => {
                    info.custom.insert(key.to_string(), value.to_string());
                }
            }
        }
        info
    }

    pub fn sample_rate(&self) -> Option<u32> {
        self.sample_rate
    }

    pub fn bitrate(&self) -> Option<u32> {
        self.bitrate
    }

    pub fn channels(&self) -> Option<u16> {
        self.channels
    }

    pub fn quality(&self) -> Option<&str> {
        self.quality.as_deref()
    }

    pub fn custom(&self) -> &HashMap<String, String> {
        &self.custom
    }
}

pub trait RequestIcyMetadata {
    fn request_icy_metadata(&mut self);
}

impl RequestIcyMetadata for HeaderMap {
    fn request_icy_metadata(&mut self) {
        self.append("Icy-MetaData", "1".parse().unwrap());
    }
}

pub struct IcyMetadataReader<T> {
    inner: T,
    icy_metaint: usize,
    next_metadata: usize,
    last_metadata_size: usize,
    current_pos: u64,
    on_metadata_read: Box<dyn Fn(Result<IcyMetadata, MetadataParseError>) + Send + Sync>,
}

impl<T> IcyMetadataReader<T> {
    pub fn new<F>(inner: T, icy_metaint: NonZeroUsize, on_metadata_read: F) -> Self
    where
        F: Fn(Result<IcyMetadata, MetadataParseError>) + Send + Sync + 'static,
    {
        Self {
            inner,
            icy_metaint: icy_metaint.get(),
            on_metadata_read: Box::new(on_metadata_read),
            next_metadata: icy_metaint.get(),
            last_metadata_size: 0,
            current_pos: 0,
        }
    }
}

// The metadata length block must be multiplied by 16 to get the total metadata length
// info taken from here https://gist.github.com/niko/2a1d7b2d109ebe7f7ca2f860c3505ef0
const ICY_METADATA_MULTIPLIER: usize = 16;

impl<T> IcyMetadataReader<T>
where
    T: Read,
{
    fn parse_metadata_from_stream(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let to_fill = buf.len();
        let mut total_written = 0;
        while total_written < to_fill {
            let prev_written = total_written;
            self.parse_next_metadata(buf, &mut total_written)?;
            // No additional data written, we're at the end of the stream
            if total_written == prev_written {
                break;
            }
        }
        self.current_pos += total_written as u64;
        Ok(total_written)
    }

    fn parse_next_metadata(&mut self, buf: &mut [u8], total_written: &mut usize) -> io::Result<()> {
        let to_fill = buf.len();

        if self.next_metadata > 0 {
            // Read data before next metadata
            let written = self.inner.read(&mut buf[..self.next_metadata])?;
            if written == 0 {
                return Ok(());
            }
            *total_written += written;
        }

        self.read_metadata()?;
        self.next_metadata = self.icy_metaint;
        let start = *total_written;

        // make sure we don't exceed the buffer length
        let end = (start + self.next_metadata).min(to_fill);
        let written = self.inner.read(&mut buf[start..end])?;
        *total_written += written;
        self.next_metadata = self.icy_metaint - written;
        Ok(())
    }

    fn read_metadata(&mut self) -> io::Result<()> {
        let mut metadata_length_buf = [0u8; 1];
        self.inner.read_exact(&mut metadata_length_buf)?;

        let metadata_length = metadata_length_buf[0] as usize * ICY_METADATA_MULTIPLIER;
        self.last_metadata_size = metadata_length + 1;
        if metadata_length > 0 {
            let mut metadata_buf = vec![0u8; metadata_length];
            self.inner.read_exact(&mut metadata_buf)?;

            let callback_val = String::from_utf8(metadata_buf)
                .map_err(MetadataParseError::InvalidUtf8)
                .and_then(|metadata_str| {
                    let metadata_str = metadata_str.trim_end_matches(char::from(0));
                    metadata_str
                        .parse::<IcyMetadata>()
                        .map_err(MetadataParseError::Empty)
                });
            (self.on_metadata_read)(callback_val);
        }
        Ok(())
    }
}

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

impl<T> Read for IcyMetadataReader<T>
where
    T: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.len() > self.next_metadata {
            self.parse_metadata_from_stream(buf)
        } else {
            let read = self.inner.read(buf)?;
            self.next_metadata -= read;
            self.current_pos += read as u64;
            Ok(read)
        }
    }
}

impl<T> Seek for IcyMetadataReader<T>
where
    T: Read + Seek,
{
    fn seek(&mut self, seek_from: io::SeekFrom) -> io::Result<u64> {
        let (requested_change, requested_pos) = match seek_from {
            SeekFrom::Start(pos) => (pos as i64 - self.current_pos as i64, pos as i64),
            SeekFrom::Current(pos) => (pos, self.current_pos as i64 + pos),
            SeekFrom::End(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "seek from end not supported",
                ));
            }
        };

        let current_absolute_pos = self.inner.stream_position()? as i64;
        let mut seek_progress = 0i64;
        if requested_change < 0 {
            let last_metadata_pos = (self.icy_metaint - self.next_metadata) as i64;
            let last_metadata_end = current_absolute_pos - last_metadata_pos;

            if current_absolute_pos + requested_change < last_metadata_end {
                self.inner.seek(SeekFrom::Current(
                    -(self.last_metadata_size as i64 + last_metadata_pos),
                ))?;
                seek_progress -= last_metadata_pos;
            }
        } else if requested_change >= self.next_metadata as i64 {
            self.inner
                .seek(SeekFrom::Current(self.next_metadata as i64))?;
            seek_progress += self.next_metadata as i64;
            self.read_metadata()?;
        }
        self.inner
            .seek(SeekFrom::Current(requested_change - seek_progress))?;
        self.next_metadata = self.icy_metaint - ((requested_pos as usize) % self.icy_metaint);
        self.current_pos = requested_pos as u64;
        Ok(self.current_pos)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IcyMetadata {
    track_title: Option<String>,
    stream_url: Option<String>,
    custom: HashMap<String, String>,
}

impl IcyMetadata {
    pub fn track_title(&self) -> Option<&str> {
        self.track_title.as_deref()
    }

    pub fn stream_url(&self) -> Option<&str> {
        self.stream_url.as_deref()
    }

    pub fn custom_fields(&self) -> &HashMap<String, String> {
        &self.custom
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyMetadataError(pub String);

impl Display for EmptyMetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "No valid values found for metadata block {}", self.0)
    }
}

impl Error for EmptyMetadataError {}

impl FromStr for IcyMetadata {
    type Err = EmptyMetadataError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut metadata = Self {
            track_title: None,
            stream_url: None,
            custom: HashMap::new(),
        };

        let ParseResult {
            map,
            errors_found,
            missing_quotes_found,
        } = parse_delimited_string(s);
        if map.is_empty() {
            return Err(EmptyMetadataError(s.to_string()));
        }

        let mut fields_found = 0;
        let mut stray_values_found = false;
        for (key, value) in map {
            fields_found += 1;
            match key.to_ascii_lowercase().as_str() {
                "streamtitle" => {
                    metadata.track_title = Some(value.to_string());
                }
                "streamurl" => {
                    metadata.stream_url = Some(value.to_string());
                }
                _ => {
                    metadata.custom.insert(key.to_string(), value.to_string());
                    stray_values_found = true;
                }
            }
        }
        // Escaping characters like quotes, semicolons, and equal signs within the metadata string
        // doesn't seem to be well-defined Here we try to handle the scenario where a stray
        // semicolon in one of the values messes with the parsing by relying on the fact
        // that StreamTitle and StreamUrl should be the only valid keys
        if errors_found || stray_values_found {
            let semicolon_count = s.chars().filter(|c| *c == ';').count();
            if semicolon_count > fields_found || missing_quotes_found {
                handle_unescaped_values(s, &mut metadata);
            }
        }

        Ok(metadata)
    }
}

fn handle_unescaped_values(s: &str, metadata: &mut IcyMetadata) {
    let stream_title_index = s
        .find("StreamTitle=")
        .or_else(|| s.find("streamTitle="))
        .or_else(|| s.find("Streamtitle="))
        .or_else(|| s.find("streamtitle="));

    let stream_url_index = s
        .find("StreamUrl=")
        .or_else(|| s.find("streamUrl="))
        .or_else(|| s.find("Streamurl="))
        .or_else(|| s.find("streamurl="));
    let (stream_title, stream_url) = match (stream_title_index, stream_url_index) {
        (Some(stream_title_index), Some(stream_url_index)) => {
            let (stream_title, stream_url) = if stream_title_index < stream_url_index {
                let stream_title = &s[stream_title_index..stream_url_index];
                let stream_url = &s[stream_url_index..];
                (stream_title, stream_url)
            } else {
                let stream_url = &s[stream_url_index..stream_title_index];
                let stream_title = &s[stream_title_index..];
                (stream_title, stream_url)
            };
            (Some(stream_title), Some(stream_url))
        }
        (Some(stream_title_index), None) => {
            let stream_title = &s[stream_title_index..];
            (Some(stream_title), None)
        }
        (None, Some(stream_url_index)) => {
            let stream_url = &s[stream_url_index..];
            (None, Some(stream_url))
        }
        (None, None) => (None, None),
    };

    if let Some(stream_title) = stream_title {
        metadata.track_title = parse_value_if_valid(stream_title);
    };

    if let Some(stream_url) = stream_url {
        metadata.stream_url = parse_value_if_valid(stream_url);
    };
}

fn parse_value_if_valid(s: &str) -> Option<String> {
    let s = if s.ends_with(';') {
        s.trim_end_matches(';')
    } else {
        s
    };
    if let (Some((_, s)), _) = parse_key_value(s) {
        Some(s.to_string())
    } else {
        None
    }
}

struct ParseResult<'a> {
    map: HashMap<&'a str, &'a str>,
    errors_found: bool,
    missing_quotes_found: bool,
}

fn parse_delimited_string(val: &str) -> ParseResult {
    let elements = val.trim().split(';');
    let mut map = HashMap::new();
    let mut errors_found = false;
    let mut missing_quotes_found = false;
    for element in elements {
        if let (Some((key, value)), missing_quotes) = parse_key_value(element) {
            map.insert(key, value);
            if missing_quotes {
                missing_quotes_found = true;
            }
        } else {
            errors_found = true;
        }
    }
    ParseResult {
        map,
        missing_quotes_found,
        errors_found,
    }
}

fn parse_key_value(val: &str) -> (Option<(&str, &str)>, bool) {
    let kv: Vec<_> = val.splitn(2, '=').collect();
    if kv.len() != 2 {
        return (None, false);
    }
    let (key, mut value) = (kv[0].trim(), kv[1].trim());
    let mut missing_quotes = false;
    if value.starts_with('\'') && value.ends_with('\'') && value.len() > 1 {
        value = &value[1..value.len() - 1];
    } else {
        missing_quotes = true;
    }
    (Some((key, value)), missing_quotes)
}
