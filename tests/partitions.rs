use std::collections::VecDeque;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

use http::HeaderMap;
use icy_metadata::{IcyHeaders, IcyMetadataReader};

struct Partitioned {
    input: Cursor<Vec<u8>>,
    parts: VecDeque<usize>,
}

impl Read for Partitioned {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let count = buf
            .len()
            .min(self.parts.front().copied().unwrap_or(buf.len()));
        let read = self.input.read(&mut buf[..count])?;
        if let Some(part) = self.parts.front_mut() {
            *part -= read;
            if *part == 0 {
                self.parts.pop_front();
            }
        }
        Ok(read)
    }
}

fn assert_partition(parts: Vec<usize>, seed: u64) {
    let data = b"abcd\x01StreamTitle='x';efgh\0ij".to_vec();
    let events = Arc::new(Mutex::new(0));
    let callback = events.clone();
    let mut reader = IcyMetadataReader::new(
        Partitioned {
            input: Cursor::new(data),
            parts: parts.into(),
        },
        NonZeroUsize::new(4),
        move |value| {
            assert_eq!(value.unwrap().stream_title(), Some("x"));
            *callback.lock().unwrap() += 1;
        },
    );
    let mut audio = Vec::new();
    reader.read_to_end(&mut audio).unwrap();
    assert_eq!(audio, b"abcdefghij", "seed={seed}");
    assert_eq!(*events.lock().unwrap(), 1, "seed={seed}");
}

#[test]
fn read_partitioned_stream() {
    let length = b"abcd\x01StreamTitle='x';efgh\0ij".len();
    for split in 1..length {
        assert_partition(vec![split, length - split], split as u64);
    }
    for seed in 1..=256u64 {
        let mut state = seed;
        let mut remaining = length;
        let mut parts = Vec::new();
        while remaining > 0 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let count = (1 + ((state >> 32) as usize % 7)).min(remaining);
            parts.push(count);
            remaining -= count;
        }
        assert_partition(parts, seed);
    }
}

#[test]
fn read_without_metadata_interval() {
    for value in [
        None,
        Some("0"),
        Some("invalid"),
        Some("184467440737095516160"),
    ] {
        let mut headers = HeaderMap::new();
        if let Some(value) = value {
            headers.insert("icy-metaint", value.parse().unwrap());
        }
        assert!(
            IcyHeaders::parse_from_headers(&headers)
                .metadata_interval()
                .is_none()
        );
    }
    let mut reader = IcyMetadataReader::new(Cursor::new(b"audio\0StreamTitle"), None, |_| panic!());
    let mut output = Vec::new();
    reader.read_to_end(&mut output).unwrap();
    assert_eq!(output, b"audio\0StreamTitle");
}

#[test]
fn read_different_metadata_intervals() {
    for (interval, data, expected) in [
        (4, b"abcd\0efgh\0".as_slice(), b"abcdefgh".as_slice()),
        (2, b"ab\0cd\0".as_slice(), b"abcd".as_slice()),
    ] {
        let mut reader =
            IcyMetadataReader::new(Cursor::new(data), NonZeroUsize::new(interval), |_| panic!());
        let mut audio = Vec::new();
        reader.read_to_end(&mut audio).unwrap();
        assert_eq!(audio, expected);
    }
}

#[test]
fn seek_from_metadata_boundary() {
    let mut reader = IcyMetadataReader::new(
        Cursor::new(b"abcd\0efgh\0ij"),
        NonZeroUsize::new(4),
        |_| panic!(),
    );
    let mut first = [0; 4];
    reader.read_exact(&mut first).unwrap();
    assert_eq!(reader.stream_position().unwrap(), 4);
    assert_eq!(reader.seek(SeekFrom::Current(4)).unwrap(), 8);
    let mut rest = Vec::new();
    reader.read_to_end(&mut rest).unwrap();
    assert_eq!(rest, b"ij");
}
