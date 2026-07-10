use anyhow::Result;

pub fn compress(data: &[u8], level: i32) -> Result<Vec<u8>> {
    Ok(zstd::encode_all(data, level)?)
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    Ok(zstd::decode_all(data)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let original = b"hello world, this is a compression test!";
        let compressed = compress(original, 6).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compression_levels() {
        let data = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let fast = compress(data, 1).unwrap();
        let best = compress(data, 9).unwrap();
        assert_eq!(decompress(&fast).unwrap(), data);
        assert_eq!(decompress(&best).unwrap(), data);
    }

    #[test]
    fn test_empty_data() {
        let compressed = compress(b"", 6).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(decompressed, b"");
    }
}
