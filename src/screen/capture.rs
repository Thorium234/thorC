use std::io;
use std::thread;
use std::time::Duration;

use scrap::{Capturer, Display};

pub struct CapturedFrame {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

pub fn capture_primary_frame() -> io::Result<CapturedFrame> {
    let display = Display::primary()?;
    let mut capturer = Capturer::new(display)?;
    let width = capturer.width();
    let height = capturer.height();

    loop {
        match capturer.frame() {
            Ok(frame) => {
                let stride = frame.len() / height;
                let mut packed = vec![0_u8; width * height * 4];

                for row in 0..height {
                    let source_start = row * stride;
                    let source_end = source_start + (width * 4);
                    let target_start = row * width * 4;
                    let target_end = target_start + (width * 4);
                    packed[target_start..target_end]
                        .copy_from_slice(&frame[source_start..source_end]);
                }

                return Ok(CapturedFrame {
                    width,
                    height,
                    data: packed,
                });
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(16));
            }
            Err(err) => return Err(err),
        }
    }
}
