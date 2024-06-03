use std::collections::HashMap;
use std::io::{self, Read, Seek, SeekFrom};
use std::num::NonZeroUsize;
use std::str::FromStr;

use crate::error::{EmptyMetadataError, MetadataParseError};
use crate::parse::{parse_delimited_string, parse_value_if_valid, ParseResult};

pub struct IcyMetadataReader<T> {
    inner: T,
    icy_metaint: Option<usize>,
    next_metadata: usize,
    last_metadata_size: usize,
    current_pos: u64,
    on_metadata_read: Box<dyn Fn(Result<IcyMetadata, MetadataParseError>) + Send + Sync>,
}

impl<T> IcyMetadataReader<T> {
    pub fn new<F>(inner: T, icy_metaint: Option<NonZeroUsize>, on_metadata_read: F) -> Self
    where
        F: Fn(Result<IcyMetadata, MetadataParseError>) + Send + Sync + 'static,
    {
        let icy_metaint = icy_metaint.map(|i| i.get());
        Self {
            inner,
            icy_metaint,
            on_metadata_read: Box::new(on_metadata_read),
            next_metadata: icy_metaint.unwrap_or(0),
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
    fn parse_metadata_from_stream(&mut self, buf: &mut [u8], metaint: usize) -> io::Result<usize> {
        let to_fill = buf.len();
        let mut total_written = 0;
        while total_written < to_fill {
            let prev_written = total_written;
            self.parse_next_metadata(buf, metaint, &mut total_written)?;
            // No additional data written, we're at the end of the stream
            if total_written == prev_written {
                break;
            }
        }
        self.current_pos += total_written as u64;
        Ok(total_written)
    }

    fn parse_next_metadata(
        &mut self,
        buf: &mut [u8],
        metaint: usize,
        total_written: &mut usize,
    ) -> io::Result<()> {
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
        self.next_metadata = metaint;
        let start = *total_written;

        // make sure we don't exceed the buffer length
        let end = (start + self.next_metadata).min(to_fill);
        let written = self.inner.read(&mut buf[start..end])?;
        *total_written += written;
        self.next_metadata = metaint - written;
        Ok(())
    }

    fn update_metadata_size(&mut self) -> io::Result<()> {
        let mut metadata_length_buf = [0u8; 1];
        self.inner.read_exact(&mut metadata_length_buf)?;

        let metadata_length = metadata_length_buf[0] as usize * ICY_METADATA_MULTIPLIER;

        self.last_metadata_size = metadata_length;
        Ok(())
    }

    fn read_metadata(&mut self) -> io::Result<()> {
        self.update_metadata_size()?;
        if self.last_metadata_size > 0 {
            let mut metadata_buf = vec![0u8; self.last_metadata_size];
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

impl<T> Read for IcyMetadataReader<T>
where
    T: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(metaint) = self.icy_metaint else {
            return self.inner.read(buf);
        };

        if buf.len() > self.next_metadata {
            self.parse_metadata_from_stream(buf, metaint)
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
        let Some(metaint) = self.icy_metaint else {
            return self.inner.seek(seek_from);
        };

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

        let mut current_absolute_pos = self.inner.stream_position()? as i64;
        let mut seek_progress = 0i64;

        if requested_change < 0 {
            let mut last_metadata_offset = (metaint - self.next_metadata) as i64;
            let mut last_metadata_end_pos = current_absolute_pos - last_metadata_offset;

            while current_absolute_pos + requested_change - seek_progress < last_metadata_end_pos
                && last_metadata_end_pos > 0
            {
                // +1 for the byte that holds the metadata length
                let metadata_region_size = self.last_metadata_size as i64 + 1;
                let seek_to = (last_metadata_end_pos - metadata_region_size) as u64;
                self.inner.seek(SeekFrom::Start(seek_to))?;
                seek_progress -= last_metadata_offset;
                self.update_metadata_size()?;
                current_absolute_pos = self.inner.seek(SeekFrom::Current(-1))? as i64;
                last_metadata_offset = metaint as i64;
                last_metadata_end_pos -= metadata_region_size + metaint as i64;
            }
        } else if requested_change >= self.next_metadata as i64 {
            self.inner
                .seek(SeekFrom::Current(self.next_metadata as i64))?;
            seek_progress += self.next_metadata as i64;
            self.read_metadata()?;
        }
        self.inner
            .seek(SeekFrom::Current(requested_change - seek_progress))?;
        self.next_metadata = metaint - ((requested_pos as usize) % metaint);
        self.current_pos = requested_pos as u64;
        Ok(self.current_pos)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    let lower_string = s.to_ascii_lowercase();
    let stream_title_index = lower_string.find("streamtitle=");
    let stream_url_index = lower_string.find("streamurl=");

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
