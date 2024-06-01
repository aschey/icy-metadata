use std::{
    collections::HashMap,
    io::{self, Read, Seek, SeekFrom},
    num::NonZeroUsize,
    str::FromStr,
};

use http::{HeaderMap, HeaderValue};

#[derive(Clone, Debug)]
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
            .map(|public| public == "1" || public.to_ascii_lowercase() == "true"),
            meta_interval: headers
                .get("icy-metaint")
                .and_then(|val| val.to_str().ok()?.to_string().parse().ok()),
            audio_info: headers.get("ice-audio-info").and_then(|val| {
                let map = parse_delimited_string(val.to_str().ok()?);
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

#[derive(Clone, Debug)]
pub struct IcyAudioInfo {
    sample_rate: Option<u32>,
    bitrate: Option<u32>,
    channels: Option<u16>,
    custom: HashMap<String, String>,
}

impl IcyAudioInfo {
    fn parse_from_map(map: HashMap<&str, &str>) -> Self {
        let mut info = Self {
            sample_rate: None,
            bitrate: None,
            channels: None,
            custom: HashMap::new(),
        };
        for (key, value) in map {
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
    on_metadata_read: Box<dyn Fn(IcyMetadata) + Send + Sync>,
}

impl<T> IcyMetadataReader<T> {
    pub fn new<F>(inner: T, icy_metaint: NonZeroUsize, on_metadata_read: F) -> Self
    where
        F: Fn(IcyMetadata) + Send + Sync + 'static,
    {
        Self {
            inner,
            icy_metaint: icy_metaint.get(),
            on_metadata_read: Box::new(on_metadata_read),
            next_metadata: icy_metaint.get(),
        }
    }
}

impl<T> Read for IcyMetadataReader<T>
where
    T: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.len() > self.next_metadata {
            let to_fill = buf.len();
            let mut total_written = 0;
            while total_written < to_fill {
                if self.next_metadata > 0 {
                    // Read data before next metadata
                    let written = self.inner.read(&mut buf[..self.next_metadata])?;
                    if written == 0 {
                        return Ok(total_written);
                    }
                    total_written += written;
                }

                // Read metadata
                let mut metadata_length_buf = [0u8; 1];
                self.inner.read_exact(&mut metadata_length_buf)?;
                let metadata_length = metadata_length_buf[0] as usize * 16;
                if metadata_length > 0 {
                    let mut metadata_buf = vec![0u8; metadata_length];
                    self.inner.read_exact(&mut metadata_buf)?;

                    if let Ok(metadata_str) = String::from_utf8(metadata_buf) {
                        // trim any null bytes at the end
                        let metadata_str = metadata_str.trim_end_matches(char::from(0));
                        if let Ok(metadata) = metadata_str.parse::<IcyMetadata>() {
                            (self.on_metadata_read)(metadata);
                        }
                    }
                }

                self.next_metadata = self.icy_metaint;
                let written = self.inner.read(
                    &mut buf[total_written..(total_written + self.next_metadata).min(to_fill)],
                )?;
                total_written += written;
                self.next_metadata = self.icy_metaint - written;
            }
            return Ok(total_written);
        }

        let read = self.inner.read(buf)?;
        self.next_metadata -= read;
        Ok(read)
    }
}

impl<T> Seek for IcyMetadataReader<T>
where
    T: Seek,
{
    fn seek(&mut self, seek_from: std::io::SeekFrom) -> std::io::Result<u64> {
        match seek_from {
            SeekFrom::Start(pos) => {
                self.next_metadata = self.icy_metaint - ((pos as usize) % self.icy_metaint);
            }
            SeekFrom::Current(pos) => {
                self.next_metadata -= (pos % self.icy_metaint as i64) as usize;
            }
            SeekFrom::End(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "seek from end not supported",
                ));
            }
        }
        self.inner.seek(seek_from)
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

impl FromStr for IcyMetadata {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut metadata = Self {
            track_title: None,
            stream_url: None,
            custom: HashMap::new(),
        };

        let map = parse_delimited_string(s);
        if map.is_empty() {
            return Err(());
        }
        for (key, value) in map {
            match key.to_ascii_lowercase().as_str() {
                "streamtitle" => {
                    metadata.track_title = Some(value.to_string());
                }
                "streamurl" => {
                    metadata.stream_url = Some(value.to_string());
                }
                _ => {
                    metadata.custom.insert(key.to_string(), value.to_string());
                }
            }
        }

        Ok(metadata)
    }
}

fn parse_delimited_string(val: &str) -> HashMap<&str, &str> {
    let elements = val.trim().split(';');
    let mut map = HashMap::new();
    for element in elements {
        let kv: Vec<_> = element.split('=').collect();
        if kv.len() != 2 {
            continue;
        }
        let (key, mut value) = (kv[0].trim(), kv[1].trim());
        if value.starts_with('\'') && value.ends_with('\'') {
            value = &value[1..value.len() - 1];
        }
        map.insert(key, value);
    }
    map
}
