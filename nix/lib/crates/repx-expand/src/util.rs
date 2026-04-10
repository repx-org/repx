use anyhow::Result;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;

const HEX: &[u8; 16] = b"0123456789abcdef";

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

pub(crate) fn write_hashed(path: &Path, data: &[u8]) -> Result<String> {
    let mut f =
        std::io::BufWriter::with_capacity(data.len().max(8192), std::fs::File::create(path)?);
    f.write_all(data)?;
    f.flush()?;

    let mut hasher = Sha256::new();
    hasher.update(data);
    Ok(hex_encode(hasher.finalize().as_slice()))
}

pub(crate) fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut file = std::io::BufReader::new(std::fs::File::open(path)?);
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex_encode(hasher.finalize().as_slice()))
}
