use std::{
    io::Read,
    num::NonZeroUsize,
    sync::{Arc, RwLock},
};

use http::HeaderMap;
use icy_metadata::{IcyHeaders, IcyMetadata, IcyMetadataReader, RequestIcyMetadata};
use rstest::rstest;

#[test]
fn read_headers() {
    let mut headers = HeaderMap::new();
    headers.append("Icy-Br", "128".parse().unwrap());
    headers.append("Icy-Genre", "genre".parse().unwrap());
    headers.append("Icy-Name", "name".parse().unwrap());
    headers.append("Icy-Url", "url".parse().unwrap());
    headers.append("Icy-Pub", "1".parse().unwrap());
    headers.append("Icy-Metaint", "16000".parse().unwrap());
    headers.append("Icy-Description", "description".parse().unwrap());
    headers.append("Icy-Notice1", "notice1".parse().unwrap());
    headers.append("Icy-Notice2", "notice2".parse().unwrap());
    headers.append(
        "Ice-Audio-Info",
        "ice-samplerate=44100;ice-bitrate=128;ice-channels=2;custom=yes"
            .parse()
            .unwrap(),
    );

    let icy_headers = IcyHeaders::parse_from_headers(&headers);
    assert_eq!(icy_headers.bitrate().unwrap(), 128);
    assert_eq!(icy_headers.genre().unwrap(), "genre");
    assert_eq!(icy_headers.stream_name().unwrap(), "name");
    assert_eq!(icy_headers.station_url().unwrap(), "url");
    assert!(icy_headers.public().unwrap());
    assert_eq!(icy_headers.meta_interval().unwrap(), 16000);
    assert_eq!(icy_headers.description().unwrap(), "description");
    assert_eq!(icy_headers.notice1().unwrap(), "notice1");
    assert_eq!(icy_headers.notice2().unwrap(), "notice2");

    let audio_info = icy_headers.audio_info().unwrap();
    assert_eq!(audio_info.sample_rate().unwrap(), 44100);
    assert_eq!(audio_info.bitrate().unwrap(), 128);
    assert_eq!(audio_info.channels().unwrap(), 2);
    assert_eq!(audio_info.custom().get("custom").unwrap(), "yes");
}

#[test]
fn add_metadata_header() {
    let mut map = HeaderMap::new();
    map.request_icy_metadata();
    assert_eq!(map.get("Icy-Metadata").unwrap().to_str().unwrap(), "1");
}

#[rstest]
fn read_stream_title(
    #[values(
        "StreamTitle='stream-title{}';",
        "StreamTitle='stream-title{}';malformedVal",
        "malformedVal;StreamTitle='stream-title{}';"
    )]
    meta_bytes: &str,
    #[values((1,0), (5,0), (5,4))] byte_lens: (usize, usize),
    #[values(1, 2)] iters: usize,
) {
    let (meta_int, trailing_bytes) = byte_lens;
    let mut data = Vec::new();
    let (mut reader, metadata) = setup_data(meta_bytes, meta_int, &mut data, iters, trailing_bytes);

    let mut buf = Vec::with_capacity(meta_int * iters + trailing_bytes);
    reader.read_to_end(&mut buf).unwrap();

    assert_eq!(buf, vec![1; buf.len()]);
    let metadata = metadata.read().unwrap();
    for i in 0..iters {
        assert_eq!(
            metadata[i].track_title().unwrap(),
            format!("stream-title{i}")
        );
    }
}

#[rstest]
fn read_stream_url(
    #[values(
        "StreamUrl='stream-url{}';",
        "StreamUrl='stream-url{}';malformedVal",
        "malformedVal;StreamUrl='stream-url{}';"
    )]
    meta_bytes: &str,
    #[values((1,0), (5,0), (5,4))] byte_lens: (usize, usize),
    #[values(1, 2)] iters: usize,
) {
    let (meta_int, trailing_bytes) = byte_lens;
    let mut data = Vec::new();
    let (mut reader, metadata) = setup_data(meta_bytes, meta_int, &mut data, iters, trailing_bytes);

    let mut buf = Vec::with_capacity(meta_int * iters + trailing_bytes);
    reader.read_to_end(&mut buf).unwrap();

    assert_eq!(buf, vec![1; buf.len()]);
    let metadata = metadata.read().unwrap();
    for i in 0..iters {
        assert_eq!(metadata[i].stream_url().unwrap(), format!("stream-url{i}"));
    }
}

#[rstest]
fn all_stream_properties(
    #[values(
        "StreamTitle='stream-title{}';StreamUrl='stream-url{}';CustomVal='custom{}';",
        "StreamTitle='stream-title{}';StreamUrl='stream-url{}';malformedVal';CustomVal='custom{}';",
        "StreamTitle='stream-title{}';malformedVal;StreamUrl='stream-url{}';CustomVal='custom{}'"
    )]
    meta_bytes: &str,
    #[values((1,0), (5,0), (5,4))] byte_lens: (usize, usize),
    #[values(1, 2)] iters: usize,
) {
    let (meta_int, trailing_bytes) = byte_lens;
    let mut data = Vec::new();
    let (mut reader, metadata) = setup_data(meta_bytes, meta_int, &mut data, iters, trailing_bytes);

    let mut buf = Vec::with_capacity(meta_int * iters + trailing_bytes);
    reader.read_to_end(&mut buf).unwrap();

    let metadata = metadata.read().unwrap();
    assert_eq!(buf, vec![1; buf.len()]);
    for i in 0..iters {
        assert_eq!(metadata[i].stream_url().unwrap(), format!("stream-url{i}"));
        assert_eq!(
            metadata[i].track_title().unwrap(),
            format!("stream-title{i}")
        );
        assert_eq!(
            metadata[i].custom_fields().get("CustomVal").unwrap(),
            &format!("custom{i}")
        );
    }
}

fn setup_data<'a>(
    meta_bytes: &str,
    meta_int: usize,
    data: &'a mut Vec<u8>,
    iters: usize,
    trailing_bytes: usize,
) -> (IcyMetadataReader<&'a [u8]>, Arc<RwLock<Vec<IcyMetadata>>>) {
    for i in 0..iters {
        let meta_bytes = meta_bytes.replace("{}", &i.to_string());
        let meta_bytes = meta_bytes.as_bytes();
        let meta_byte = meta_bytes.len() / 16 + 1;

        data.extend_from_slice(vec![1; meta_int].as_slice());
        data.push(meta_byte as u8);
        data.extend_from_slice(meta_bytes);
        let padding = vec![0; meta_byte * 16 - meta_bytes.len()];

        data.extend_from_slice(&padding);
    }
    data.extend_from_slice(&vec![1; trailing_bytes]);

    let metadata = Arc::new(RwLock::new(vec![]));
    let reader = {
        let metadata = metadata.clone();
        IcyMetadataReader::new(
            data.as_slice(),
            NonZeroUsize::new(meta_int).unwrap(),
            move |meta| {
                metadata.write().unwrap().push(meta);
            },
        )
    };
    (reader, metadata)
}

#[rstest]
fn read_larger_than_stream_size(
    #[values(
        "StreamUrl='stream-url{}';",
        "StreamUrl='stream-url{}';malformedVal",
        "malformedVal;StreamUrl='stream-url{}';"
    )]
    meta_bytes: &str,
    #[values((10,5))] byte_lens: (usize, usize),
    #[values(1, 2)] iters: usize,
) {
    let (meta_int, trailing_bytes) = byte_lens;
    let mut data = Vec::new();
    let (mut reader, metadata) = setup_data(meta_bytes, meta_int, &mut data, iters, trailing_bytes);

    let stream_size = meta_int * iters + trailing_bytes;
    let mut buf = vec![0; stream_size + 1];
    let read_amount = reader.read(&mut buf).unwrap();
    assert_eq!(read_amount, stream_size);
    assert_eq!(buf[..read_amount], vec![1; read_amount]);

    let metadata = metadata.read().unwrap();
    for i in 0..iters {
        assert_eq!(metadata[i].stream_url().unwrap(), format!("stream-url{i}"));
    }
}

#[rstest]
fn small_reads(
    #[values(
        "StreamUrl='stream-url{}';",
        "StreamUrl='stream-url{}';malformedVal",
        "malformedVal;StreamUrl='stream-url{}';"
    )]
    meta_bytes: &str,
    #[values((10,5))] byte_lens: (usize, usize),
    #[values(1, 2)] iters: usize,
) {
    let (meta_int, trailing_bytes) = byte_lens;
    let mut data = Vec::new();
    let (mut reader, metadata) = setup_data(meta_bytes, meta_int, &mut data, iters, trailing_bytes);

    let stream_size = meta_int * iters + trailing_bytes;
    let mut buf = vec![0; stream_size];

    let mut read_amount = 0;
    while read_amount < buf.len() {
        read_amount += reader.read(&mut buf[read_amount..read_amount + 1]).unwrap();
    }
    assert_eq!(read_amount, stream_size);
    assert_eq!(buf[..read_amount], vec![1; read_amount]);

    let metadata = metadata.read().unwrap();
    for i in 0..iters {
        assert_eq!(metadata[i].stream_url().unwrap(), format!("stream-url{i}"));
    }
}

#[rstest]
fn empty_metadata(
    #[values("")] meta_bytes: &str,
    #[values((1,0), (5,0), (5,4))] byte_lens: (usize, usize),
    #[values(1, 2)] iters: usize,
) {
    let (meta_int, trailing_bytes) = byte_lens;
    let mut data = Vec::new();
    let (mut reader, metadata) = setup_data(meta_bytes, meta_int, &mut data, iters, trailing_bytes);

    let mut buf = Vec::with_capacity(meta_int * iters + trailing_bytes);
    reader.read_to_end(&mut buf).unwrap();

    assert_eq!(buf, vec![1; buf.len()]);
    let metadata = metadata.read().unwrap();
    assert_eq!(metadata.len(), 0);
}
