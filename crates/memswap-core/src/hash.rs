/// Content-addressed hashing. All hashing in memswap is blake3.
pub fn blake3_hex(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}
