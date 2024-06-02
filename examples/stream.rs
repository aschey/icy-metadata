use std::error::Error;
use std::num::NonZeroUsize;

use icy_metadata::{IcyHeaders, IcyMetadataReader, RequestIcyMetadata};
use stream_download::http::reqwest::{self, Client};
use stream_download::http::HttpStream;
use stream_download::storage::bounded::BoundedStorageProvider;
use stream_download::storage::memory::MemoryStorageProvider;
use stream_download::{Settings, StreamDownload};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let (_stream, handle) = rodio::OutputStream::try_default()?;
    let sink = rodio::Sink::try_new(&handle)?;

    let mut headers = reqwest::header::HeaderMap::new();
    headers.request_icy_metadata();
    let client = Client::builder().default_headers(headers).build()?;

    let stream =
        HttpStream::new(client, "https://ice6.somafm.com/insound-128-mp3".parse()?).await?;

    let icy_headers = IcyHeaders::parse_from_headers(stream.headers());
    println!("Icecast headers: {icy_headers:#?}\n");
    println!("content type={:?}\n", stream.content_type());

    // buffer 5 seconds of audio
    // bitrate (in kilobits) / bits per byte * bytes per kilobyte * 5 seconds
    let prefetch_bytes = icy_headers.bitrate().unwrap() / 8 * 1024 * 5;

    let reader = StreamDownload::from_stream(
        stream,
        // use bounded storage to keep the underlying size from growing indefinitely
        BoundedStorageProvider::new(
            MemoryStorageProvider,
            // be liberal with the buffer size, you need to make sure it holds enough space to
            // prevent any out-of-bounds reads
            NonZeroUsize::new(512 * 1024).unwrap(),
        ),
        Settings::default().prefetch_bytes(prefetch_bytes as u64),
    )
    .await?;
    sink.append(rodio::Decoder::new(IcyMetadataReader::new(
        reader,
        icy_headers.meta_interval(),
        |metadata| println!("{metadata:#?}\n"),
    ))?);

    let handle = tokio::task::spawn_blocking(move || {
        sink.sleep_until_end();
    });
    handle.await?;
    Ok(())
}
