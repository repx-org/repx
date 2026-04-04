use sha2::{Digest, Sha256};

const NIX32_CHARS: &[u8; 32] = b"0123456789abcdfghijklmnpqrsvwxyz";

const HASH_LEN: usize = 32;
const NIX32_FULL_LEN: usize = 52;

const NIX_SEPARATOR: &[u8] = b"x00";

pub const JOB_ID_LEN: usize = 32;

pub type JobId = [u8; JOB_ID_LEN];

#[inline]
fn encode_nix32(hash: &[u8; HASH_LEN]) -> [u8; NIX32_FULL_LEN] {
    let mut out = [0u8; NIX32_FULL_LEN];
    let hash_bits = HASH_LEN * 8;

    for n in (0..NIX32_FULL_LEN).rev() {
        let b = n * 5;
        let mut c: u8 = 0;
        for bit in 0..5 {
            let pos = b + bit;
            if pos < hash_bits {
                let byte_idx = pos / 8;
                let bit_idx = pos % 8;
                if (hash[byte_idx] >> bit_idx) & 1 == 1 {
                    c |= 1 << bit;
                }
            }
        }
        out[NIX32_FULL_LEN - 1 - n] = NIX32_CHARS[c as usize];
    }

    out
}

pub struct JobIdHasher {
    inner: Sha256,
    needs_sep: bool,
}

impl JobIdHasher {
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: Sha256::new(),
            needs_sep: false,
        }
    }

    #[inline]
    pub fn feed(&mut self, data: &[u8]) {
        if self.needs_sep {
            self.inner.update(NIX_SEPARATOR);
        }
        self.inner.update(data);
        self.needs_sep = true;
    }

    #[inline]
    pub fn feed_str(&mut self, s: &str) {
        self.feed(s.as_bytes());
    }

    #[inline]
    pub fn finish(self) -> JobId {
        let digest: [u8; HASH_LEN] = self.inner.finalize().into();
        let full = encode_nix32(&digest);
        let mut id = [0u8; JOB_ID_LEN];
        id.copy_from_slice(&full[..JOB_ID_LEN]);
        id
    }

    #[inline]
    pub fn reset(&mut self) {
        self.inner = Sha256::new();
        self.needs_sep = false;
    }
}

#[inline]
pub fn job_id_str(id: &JobId) -> &str {
    unsafe { std::str::from_utf8_unchecked(id) }
}

pub fn mk_job_id(hash_inputs: &[&str]) -> JobId {
    let mut hasher = JobIdHasher::new();
    for input in hash_inputs {
        hasher.feed_str(input);
    }
    hasher.finish()
}

pub fn sha256_hex(input: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(input);
    let digest = h.finalize();
    let mut s = String::with_capacity(64);
    for &b in digest.as_slice() {
        s.push(HEX_CHARS[(b >> 4) as usize] as char);
        s.push(HEX_CHARS[(b & 0x0f) as usize] as char);
    }
    s
}

const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nix32_encoding_empty_hash() {
        let mut h = Sha256::new();
        h.update(b"");
        let digest: [u8; 32] = h.finalize().into();
        let nix32 = encode_nix32(&digest);
        let s = std::str::from_utf8(&nix32).unwrap();
        assert_eq!(s, "0mdqa9w1p6cmli6976v4wi0sw9r4p5prkj7lzfd1877wk11c9c73");
    }

    #[test]
    fn test_mk_job_id_matches_nix() {
        let id = mk_job_id(&["hello", "world"]);
        let id_str = job_id_str(&id);

        let mut h = Sha256::new();
        h.update(b"hellox00world");
        let digest: [u8; 32] = h.finalize().into();
        let full = encode_nix32(&digest);
        let expected = std::str::from_utf8(&full[..32]).unwrap();

        assert_eq!(id_str, expected);
    }

    #[test]
    fn test_mk_job_id_single_input_no_separator() {
        let id = mk_job_id(&["hello"]);
        let id_str = job_id_str(&id);

        let mut h = Sha256::new();
        h.update(b"hello");
        let digest: [u8; 32] = h.finalize().into();
        let full = encode_nix32(&digest);
        let expected = std::str::from_utf8(&full[..32]).unwrap();

        assert_eq!(id_str, expected);
    }

    #[test]
    fn test_incremental_matches_batch() {
        let batch = mk_job_id(&["aaa", "bbb", "ccc"]);

        let mut hasher = JobIdHasher::new();
        hasher.feed_str("aaa");
        hasher.feed_str("bbb");
        hasher.feed_str("ccc");
        let incremental = hasher.finish();

        assert_eq!(batch, incremental);
    }

    #[test]
    fn test_hasher_reuse() {
        let mut hasher = JobIdHasher::new();
        hasher.feed_str("first");
        let id1 = hasher.finish();

        let mut hasher2 = JobIdHasher::new();
        hasher2.feed_str("first");
        let id2 = hasher2.finish();

        assert_eq!(id1, id2);
    }

    #[test]
    fn test_deterministic() {
        let a = mk_job_id(&["x", "y"]);
        let b = mk_job_id(&["x", "y"]);
        assert_eq!(a, b);
    }

    #[test]
    fn test_different_inputs_differ() {
        let a = mk_job_id(&["x", "y"]);
        let b = mk_job_id(&["y", "x"]);
        assert_ne!(a, b);
    }

    #[test]
    fn test_job_id_is_valid_ascii() {
        let id = mk_job_id(&["test", "data", "here"]);
        let s = job_id_str(&id);
        assert_eq!(s.len(), 32);
        for c in s.bytes() {
            assert!(
                NIX32_CHARS.contains(&c),
                "Invalid nix32 char: {}",
                c as char
            );
        }
    }
}
