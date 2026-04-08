pub fn encode_frame(width: usize, height: usize, frame: &[u8]) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(8 + frame.len());
    encoded.extend_from_slice(&(width as u32).to_le_bytes());
    encoded.extend_from_slice(&(height as u32).to_le_bytes());
    encoded.extend_from_slice(frame);
    encoded
}
