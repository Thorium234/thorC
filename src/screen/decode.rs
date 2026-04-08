use std::io;

pub struct DecodedFrame {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

pub fn decode_frame(data: &[u8]) -> io::Result<DecodedFrame> {
    if data.len() < 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame payload is too small",
        ));
    }

    let width = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let height = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    let pixels = &data[8..];

    if pixels.len() != width * height * 4 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame size does not match declared dimensions",
        ));
    }

    let mut rgba = Vec::with_capacity(pixels.len());
    for chunk in pixels.chunks_exact(4) {
        rgba.push(chunk[2]);
        rgba.push(chunk[1]);
        rgba.push(chunk[0]);
        rgba.push(255);
    }

    Ok(DecodedFrame {
        width,
        height,
        rgba,
    })
}
