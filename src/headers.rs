use std::collections::HashMap;
use std::num::NonZeroUsize;

use http::HeaderMap;

use crate::parse::{ParseResult, parse_delimited_string};

/// Header name to request icy metadata.
pub const ICY_METADATA_HEADER: &str = "Icy-MetaData";

/// Appends the `Icy-MetaData` header to the `header_map`.
///
/// # Panics
///
/// This method panics if the header map size is exceeded.
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

/// Icy metadata found within HTTP response headers.
#[derive(Clone, Debug, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IcyHeaders {
    bitrate: Option<u32>,
    sample_rate: Option<u32>,
    genre: Vec<String>,
    name: Option<String>,
    station_url: Option<String>,
    description: Option<String>,
    public: Option<bool>,
    notice1: Option<String>,
    notice2: Option<String>,
    loudness: Option<f32>,
    logo_url: Option<String>,
    main_stream_url: Option<String>,
    version: Option<u32>,
    index_metadata: Option<bool>,
    country_code: Option<String>,
    country_subdivision_code: Option<String>,
    language_codes: Vec<String>,
    geo_lat_long: Option<[f32; 2]>,
    do_not_index: Option<bool>,
    metadata_interval: Option<NonZeroUsize>,
    audio_info: Option<IcyAudioInfo>,
}

fn find_header(search: &[&str], headers: &HeaderMap) -> Option<String> {
    for header in search {
        if let Some(val) = headers.get(*header) {
            return val.to_str().ok().map(|s| s.trim_ascii().to_string());
        }
    }
    None
}

fn str_to_bool(val: &str) -> bool {
    // 1 and 0 are the only typical values, but we'll look for a few other truthy values
    val == "1" || val.eq_ignore_ascii_case("true") || val.eq_ignore_ascii_case("yes")
}

fn comma_separated(val: String) -> Vec<String> {
    val.split(',').map(|s| s.trim_ascii().to_string()).collect()
}

impl IcyHeaders {
    /// Parse any icy metadata contained in the `headers`.
    pub fn parse_from_headers(headers: &HeaderMap) -> Self {
        // Most header names taken from here https://github.com/xiph/Icecast-Server/blob/master/src/source.c
        Self {
            bitrate: find_header(&["ice-bitrate", "icy-br", "x-audiocast-bitrate"], headers)
                // sometimes there are multiple values here, we'll just take the first one
                .and_then(|val| comma_separated(val).first()?.parse().ok()),
            // Note: this isn't included in the Icecast-Server repo, but I've seen a few servers
            // include icy-sr as a header. Unclear if the other aliases here are
            // actually used at all
            sample_rate: find_header(
                &["ice-samplerate", "icy-sr", "x-audiocast-samplerate"],
                headers,
            )
            .and_then(|val| val.parse().ok()),
            genre: find_header(&["ice-genre", "icy-genre", "x-audiocast-genre"], headers)
                .map(comma_separated)
                .unwrap_or_default(),
            name: find_header(&["ice-name", "icy-name", "x-audiocast-name"], headers),
            description: find_header(
                &[
                    "ice-description",
                    "icy-description",
                    "x-audiocast-description",
                ],
                headers,
            ),
            station_url: find_header(&["ice-url", "icy-url", "x-audiocast-url"], headers),
            notice1: find_header(
                &["ice-notice1", "icy-notice1", "x-audiocast-notice1"],
                headers,
            ),
            notice2: find_header(
                &["ice-notice2", "icy-notice2", "x-audiocast-notice2"],
                headers,
            ),
            // I can't find any documentation on this header, but some servers return it
            loudness: find_header(&["X-Loudness"], headers).and_then(|val| val.parse().ok()),
            public: find_header(
                &["ice-public", "icy-pub", "icy-public", "x-audiocast-public"],
                headers,
            )
            .as_deref()
            .map(str_to_bool),
            logo_url: find_header(&["icy-logo"], headers),
            main_stream_url: find_header(&["icy-main-stream-url"], headers),
            version: find_header(&["icy-version"], headers).and_then(|h| h.parse().ok()),
            index_metadata: find_header(&["icy-index-metadata"], headers)
                .as_deref()
                .map(str_to_bool),
            country_code: find_header(&["icy-country-code"], headers),
            country_subdivision_code: find_header(&["icy-country-subdivision-code"], headers),
            language_codes: find_header(&["icy-language-codes", "icy-language-code"], headers)
                .map(comma_separated)
                .unwrap_or_default(),
            geo_lat_long: find_header(&["icy-geo-lat-long"], headers).and_then(|h| {
                if let [lat, long] = &comma_separated(h)[..] {
                    if let (Ok(lat), Ok(long)) = (lat.parse(), long.parse()) {
                        return Some([lat, long]);
                    }
                }
                None
            }),
            do_not_index: find_header(&["icy-do-not-index"], headers)
                .as_deref()
                .map(str_to_bool),
            metadata_interval: find_header(&["icy-metaint"], headers)
                .and_then(|val| NonZeroUsize::new(val.parse().ok()?)),
            audio_info: find_header(&["ice-audio-info", "icy-audio-info"], headers).map(|val| {
                let ParseResult { map, .. } = parse_delimited_string(&val);
                IcyAudioInfo::parse_from_map(map)
            }),
        }
    }

    fn audio_info_prop<F, T>(&self, f: F) -> Option<T>
    where
        F: Fn(&IcyAudioInfo) -> Option<T>,
    {
        self.audio_info.as_ref().and_then(f)
    }

    /// Stream bitrate.
    pub fn bitrate(&self) -> Option<u32> {
        self.bitrate.or(self.audio_info_prop(|a| a.bitrate))
    }

    /// Stream sample rate.
    pub fn sample_rate(&self) -> Option<u32> {
        self.sample_rate.or(self.audio_info_prop(|a| a.sample_rate))
    }

    /// Stream genre.
    pub fn genre(&self) -> &[String] {
        &self.genre
    }

    /// Stream description.
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Stream name.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Stream station URL.
    pub fn station_url(&self) -> Option<&str> {
        self.station_url.as_deref()
    }

    /// You probably don't care about this, but it's here just in case.
    /// If it's set, it might say something like `<BR>This stream requires <a href="http://www.winamp.com">Winamp</a><BR>`.
    pub fn notice1(&self) -> Option<&str> {
        self.notice1.as_deref()
    }

    /// You probably don't care about this, but it's here just in case.
    /// If it's set, it might contain the Icecast/Shoutcast server version.
    pub fn notice2(&self) -> Option<&str> {
        self.notice2.as_deref()
    }

    /// Loudness normalization info.
    pub fn loudness(&self) -> Option<f32> {
        self.loudness
    }

    /// Whether the stream is listed or not.
    pub fn public(&self) -> Option<bool> {
        self.public
    }

    /// Number of channels in the stream.
    pub fn channels(&self) -> Option<u16> {
        self.audio_info_prop(|a| a.channels)
    }

    /// Stream quality.
    pub fn quality(&self) -> Option<String> {
        self.audio_info_prop(|a| a.quality.clone())
    }

    /// URL of the logo for she stream.
    pub fn logo_url(&self) -> Option<&str> {
        self.logo_url.as_deref()
    }

    /// The main URL for this stream.
    pub fn main_stream_url(&self) -> Option<&str> {
        self.main_stream_url.as_deref()
    }

    /// Version of the metadata spec. 1 is the default and 2 contains additional properties.
    pub fn version(&self) -> Option<u32> {
        self.version
    }

    /// An extra property that can be set to true to specify that the metadata is set correctly and
    /// isn't just left as the default values.
    pub fn index_metadata(&self) -> Option<bool> {
        self.index_metadata
    }

    /// 2-letter country code for the stream. Values are specified in
    /// [ISO 3166-1](https://en.wikipedia.org/wiki/ISO_3166-1_alpha-2).
    pub fn country_code(&self) -> Option<&str> {
        self.country_code.as_deref()
    }

    /// Code for the subdivision of the stream country. Values are specified in
    /// [ISO 3166-2](https://en.wikipedia.org/wiki/ISO_3166-2).
    pub fn country_subdivision_code(&self) -> Option<&str> {
        self.country_subdivision_code.as_deref()
    }

    /// Language codes used by the stream. Can be a 2-letter code specified in
    /// [ISO 639-1](https://en.wikipedia.org/wiki/List_of_ISO_639_language_codes) or
    /// a 3-letter code specified in
    /// [ISO 639-3](https://en.wikipedia.org/wiki/ISO_639-3).
    pub fn language_codes(&self) -> &[String] {
        &self.language_codes
    }

    /// Latitude and longitude of the stream.
    pub fn geo_lat_long(&self) -> Option<[f32; 2]> {
        self.geo_lat_long
    }

    /// Can be set to true if the stream operator wants it to be private.
    pub fn do_not_index(&self) -> Option<bool> {
        self.do_not_index
    }

    /// Additional properties, if available.
    pub fn custom(&self) -> HashMap<String, String> {
        self.audio_info_prop(|a| Some(a.custom.clone()))
            .unwrap_or_default()
    }

    /// This will only be set if the stream was requested with the `Icy-MetaInt` header set to `1`.
    /// Use the convenience functions in this crate to set this, or add the header yourself.
    /// This needs to be passed in to [`IcyMetadataReader::new`](crate::IcyMetadataReader::new) in
    /// order to read the metadata.
    pub fn metadata_interval(&self) -> Option<NonZeroUsize> {
        self.metadata_interval
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct IcyAudioInfo {
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
            let key = key.trim_ascii();
            let value = value.trim_ascii();
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
                    info.quality = value.into_owned().into();
                }
                _ => {
                    info.custom.insert(key.to_string(), value.to_string());
                }
            }
        }
        info
    }
}
