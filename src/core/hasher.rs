use sha2::{Digest, Sha256};

pub fn hash_buffer(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

pub fn hash_string(data: &str) -> String {
    hash_buffer(data.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_buffer_empty() {
        let result = hash_buffer(b"");
        assert_eq!(
            result,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_hash_buffer_hello() {
        let result = hash_buffer(b"hello");
        assert_eq!(
            result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_hash_string() {
        let result = hash_string("hello");
        assert_eq!(result, hash_buffer(b"hello"));
    }
}
