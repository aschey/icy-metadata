use std::collections::HashMap;
use std::num::NonZeroUsize;

use http::{HeaderMap, HeaderValue};

use crate::parse::{parse_delimited_string, ParseResult};

#[derive(Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IcyHeaders {
    bitrate: Option<u32>,
    genre: Option<String>,
    stream_name: Option<String>,
    station_url: Option<String>,
    description: Option<String>,
    public: Option<bool>,
    notice1: Option<String>,
    notice2: Option<String>,
    meta_interval: Option<NonZeroUsize>,
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
                .and_then(|val| NonZeroUsize::new(val.to_str().ok()?.to_string().parse().ok()?)),
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

    pub fn meta_interval(&self) -> Option<NonZeroUsize> {
        self.meta_interval
    }

    pub fn audio_info(&self) -> Option<&IcyAudioInfo> {
        self.audio_info.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
