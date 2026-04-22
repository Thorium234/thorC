/// Represents a changed rectangular region in a frame.
#[derive(Debug, Clone)]
pub struct DeltaRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Configuration for delta detection.
#[derive(Debug, Clone)]
pub struct DeltaConfig {
    /// Size of grid cells used for change detection (pixels).
    pub cell_size: u32,
    /// Minimum number of changed cells before sending full frame.
    pub full_frame_threshold: f64,
}

impl Default for DeltaConfig {
    fn default() -> Self {
        Self {
            cell_size: 32,
            full_frame_threshold: 0.5,
        }
    }
}

/// Detects changes between two frames and produces delta regions.
pub struct DeltaEncoder {
    config: DeltaConfig,
    last_frame: Option<Vec<u8>>,
    last_width: usize,
    last_height: usize,
}

impl DeltaEncoder {
    pub fn new(config: DeltaConfig) -> Self {
        Self {
            config,
            last_frame: None,
            last_width: 0,
            last_height: 0,
        }
    }

    /// Compare the current frame with the previous one and return delta regions.
    ///
    /// Returns `None` if this is the first frame (no previous to compare against),
    /// in which case the caller should send a full frame.
    /// Returns `Some(Vec<DeltaRegion>)` with only the changed regions.
    /// Returns `Some(vec![])` if no changes were detected.
    pub fn compute_delta(
        &mut self,
        frame: &[u8],
        width: usize,
        height: usize,
    ) -> Option<Vec<DeltaRegion>> {
        let bytes_per_pixel = 4;
        let row_stride = width * bytes_per_pixel;

        // First frame: store it and signal that a full frame is needed
        if self.last_frame.is_none() {
            self.last_frame = Some(frame.to_vec());
            self.last_width = width;
            self.last_height = height;
            return None;
        }

        // If dimensions changed, force full frame
        if width != self.last_width || height != self.last_height {
            self.last_frame = Some(frame.to_vec());
            self.last_width = width;
            self.last_height = height;
            return None;
        }

        let last = self.last_frame.as_ref().expect("last frame exists");
        let cell_size = self.config.cell_size as usize;
        let cells_x = (width + cell_size - 1) / cell_size;
        let cells_y = (height + cell_size - 1) / cell_size;
        let total_cells = cells_x * cells_y;
        let mut changed_cells = 0usize;
        let mut regions = Vec::new();

        for cy in 0..cells_y {
            for cx in 0..cells_x {
                let x = cx * cell_size;
                let y = cy * cell_size;
                let w = cell_size.min(width - x);
                let h = cell_size.min(height - y);

                if self.cells_differ(last, frame, width, x, y, w, h) {
                    changed_cells += 1;
                    // Extract the changed region data
                    let mut region_data = Vec::with_capacity(w * h * bytes_per_pixel);
                    for row in y..(y + h) {
                        let start = row * row_stride + x * bytes_per_pixel;
                        let end = start + w * bytes_per_pixel;
                        region_data.extend_from_slice(&frame[start..end]);
                    }
                    regions.push(DeltaRegion {
                        x: x as u32,
                        y: y as u32,
                        width: w as u32,
                        height: h as u32,
                        data: region_data,
                    });
                }
            }
        }

        // Update last frame
        self.last_frame = Some(frame.to_vec());

        // If too many cells changed, signal caller to send full frame instead
        let change_ratio = changed_cells as f64 / total_cells as f64;
        if change_ratio > self.config.full_frame_threshold {
            return None;
        }

        Some(regions)
    }

    /// Check if a cell region differs between two frames.
    fn cells_differ(
        &self,
        a: &[u8],
        b: &[u8],
        width: usize,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
    ) -> bool {
        let bytes_per_pixel = 4;
        let row_stride = width * bytes_per_pixel;
        for row in y..(y + h) {
            let start = row * row_stride + x * bytes_per_pixel;
            let end = start + w * bytes_per_pixel;
            if a[start..end] != b[start..end] {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
pub fn compress_frame(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(data)
        .expect("compression should not fail");
    encoder.finish().expect("compression should not fail")
}

#[cfg(test)]
pub fn decompress_frame(data: &[u8]) -> std::io::Result<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    let mut decoder = ZlibDecoder::new(data);
    let mut result = Vec::new();
    decoder.read_to_end(&mut result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_frame(width: usize, height: usize, color: u8) -> Vec<u8> {
        vec![color; width * height * 4]
    }

    #[test]
    fn test_first_frame_returns_none() {
        let mut encoder = DeltaEncoder::new(DeltaConfig::default());
        let frame = make_test_frame(64, 64, 0xFF);
        let result = encoder.compute_delta(&frame, 64, 64);
        assert!(result.is_none());
    }

    #[test]
    fn test_unchanged_frame_returns_empty() {
        let mut encoder = DeltaEncoder::new(DeltaConfig::default());
        let frame = make_test_frame(64, 64, 0xFF);
        encoder.compute_delta(&frame, 64, 64); // first frame

        let result = encoder.compute_delta(&frame, 64, 64);
        assert!(result.is_some());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_changed_region_detected() {
        let mut encoder = DeltaEncoder::new(DeltaConfig::default());
        let frame1 = make_test_frame(64, 64, 0xFF);
        encoder.compute_delta(&frame1, 64, 64);

        let mut frame2 = frame1.clone();
        // Change a small region in the top-left cell
        for i in 0..16 {
            for j in 0..16 {
                let idx = (i * 64 + j) * 4;
                frame2[idx] = 0x00;
            }
        }

        let result = encoder.compute_delta(&frame2, 64, 64);
        assert!(result.is_some());
        let regions = result.unwrap();
        assert!(!regions.is_empty());
    }

    #[test]
    fn test_compress_decompress_roundtrip() {
        let data = b"hello world, this is test frame data";
        let compressed = compress_frame(data);
        assert!(!compressed.is_empty());
        let decompressed = decompress_frame(&compressed).unwrap();
        assert_eq!(&decompressed, data);
    }
}
