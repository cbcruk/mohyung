use anyhow::Result;
use flate2::read::{GzDecoder, GzEncoder};
use flate2::Compression;
use std::io::Read;

pub fn compress(data: &[u8], level: u32) -> Vec<u8> {
    let mut encoder = GzEncoder::new(data, Compression::new(level));
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed).expect("gzip compression failed");
    compressed
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let original = b"hello world, this is a compression test!";
        let compressed = compress(original, 6);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compression_levels() {
        let data = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let fast = compress(data, 1);
        let best = compress(data, 9);
        assert!(fast.len() <= data.len());
        assert!(best.len() <= data.len());
    }

    #[test]
    fn test_empty_data() {
        let compressed = compress(b"", 6);
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, b"");
    }
}
