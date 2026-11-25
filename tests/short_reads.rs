use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

use icy_metadata::IcyMetadataReader;

struct Fragmented {
    inner: Cursor<Vec<u8>>,
    maximum: usize,
    fault: Option<(usize, io::ErrorKind)>,
}

impl Read for Fragmented {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if let Some((position, kind)) = self.fault {
            if self.inner.position() as usize == position {
                self.fault = None;
                return Err(kind.into());
            }
        }
        let mut limit = buf.len().min(self.maximum);
        if let Some((position, _)) = self.fault {
            limit = limit.min(position.saturating_sub(self.inner.position() as usize));
        }
        self.inner.read(&mut buf[..limit])
    }
}

impl Seek for Fragmented {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.inner.seek(position)
    }
}

#[test]
fn seek_with_incomplete_metadata() {
    let input = Fragmented {
        inner: Cursor::new(wire(1)),
        maximum: 3,
        fault: Some((8, io::ErrorKind::WouldBlock)),
    };
    let mut reader = IcyMetadataReader::new(input, NonZeroUsize::new(4), |value| {
        assert_eq!(value.unwrap().stream_title(), Some("x"));
    });
    let mut first = [0; 4];
    reader.read_exact(&mut first).unwrap();
    assert_eq!(&first, b"abcd");
    assert_eq!(
        reader.read(&mut [0; 32]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(reader.stream_position().unwrap(), 4);
    assert_eq!(
        reader.seek(SeekFrom::Start(0)).unwrap_err().kind(),
        io::ErrorKind::Unsupported
    );
    let mut rest = Vec::new();
    reader.read_to_end(&mut rest).unwrap();
    assert_eq!(rest, b"efghij");
    assert_eq!(reader.stream_position().unwrap(), 10);
}

#[test]
fn read_after_seek_error() {
    let input = Fragmented {
        inner: Cursor::new(wire(1)),
        maximum: 3,
        fault: Some((8, io::ErrorKind::WouldBlock)),
    };
    let mut reader = IcyMetadataReader::new(input, NonZeroUsize::new(4), |value| {
        assert_eq!(value.unwrap().stream_title(), Some("x"));
    });
    assert_eq!(
        reader.seek(SeekFrom::Start(6)).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(reader.stream_position().unwrap(), 4);
    let mut audio = Vec::new();
    reader.read_to_end(&mut audio).unwrap();
    assert_eq!(audio, b"efghij");
}

fn wire(units: u8) -> Vec<u8> {
    let mut data = b"abcd".to_vec();
    data.push(units);
    if units != 0 {
        let mut metadata = b"StreamTitle='x';".to_vec();
        metadata.resize(usize::from(units) * 16, 0);
        data.extend(metadata);
    }
    data.extend(b"efgh\0ij");
    data
}

fn run(
    data: Vec<u8>,
    maximum: usize,
    caller: usize,
    fault: Option<(usize, io::ErrorKind)>,
) -> (Vec<u8>, usize) {
    let events = Arc::new(Mutex::new(0));
    let callback = events.clone();
    let input = Fragmented {
        inner: Cursor::new(data),
        maximum,
        fault,
    };
    let mut reader = IcyMetadataReader::new(input, NonZeroUsize::new(4), move |metadata| {
        assert_eq!(metadata.unwrap().stream_title(), Some("x"));
        *callback.lock().unwrap() += 1;
    });
    let mut output = Vec::new();
    let mut buf = vec![0; caller];
    for _ in 0..20000 {
        assert_eq!(reader.read(&mut []).unwrap(), 0);
        match reader.read(&mut buf) {
            Ok(0) => return (output, *events.lock().unwrap()),
            Ok(count) => output.extend_from_slice(&buf[..count]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            Err(error) => panic!("framing error: {error}; audio={output:?}"),
        }
    }
    panic!("reader failed to make bounded progress");
}

#[test]
fn one_byte_reads() {
    let (audio, events) = run(b"abcd\0efgh\0".to_vec(), 1, 32, None);
    assert_eq!(audio, b"abcdefgh");
    assert_eq!(events, 0);
}

#[test]
fn short_reads() {
    for units in [0, 1, 255] {
        for maximum in 1..=17 {
            for caller in [1, 2, 3, 4, 5, 7, 32, 8192] {
                let (audio, events) = run(wire(units), maximum, caller, None);
                assert_eq!(audio, b"abcdefghij");
                assert_eq!(events, usize::from(units != 0));
            }
        }
    }
}

#[test]
fn read_with_temporary_errors() {
    let data = wire(1);
    for position in 0..data.len() {
        for kind in [io::ErrorKind::Interrupted, io::ErrorKind::WouldBlock] {
            let (audio, events) = run(data.clone(), 3, 32, Some((position, kind)));
            assert_eq!(audio, b"abcdefghij", "position={position}, kind={kind:?}");
            assert_eq!(events, 1);
        }
    }
}

#[test]
fn truncated_metadata() {
    for cut in 4..21 {
        let mut reader = IcyMetadataReader::new(
            Cursor::new(wire(1)[..cut].to_vec()),
            NonZeroUsize::new(4),
            |_| panic!("truncated block emitted metadata"),
        );
        let mut buf = [0; 64];
        assert_eq!(reader.read(&mut buf).unwrap(), 4);
        assert_eq!(&buf[..4], b"abcd");
        assert_eq!(
            reader.read(&mut buf).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}

#[test]
fn invalid_metadata_utf8() {
    let mut data = wire(1);
    data[5] = 255;
    let errors = Arc::new(Mutex::new(0));
    let callback = errors.clone();
    let mut reader =
        IcyMetadataReader::new(Cursor::new(data), NonZeroUsize::new(4), move |value| {
            assert!(value.is_err());
            *callback.lock().unwrap() += 1;
        });
    let mut audio = Vec::new();
    reader.read_to_end(&mut audio).unwrap();
    assert_eq!(audio, b"abcdefghij");
    assert_eq!(*errors.lock().unwrap(), 1);
}
