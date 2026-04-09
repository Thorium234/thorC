use std::io;
use std::thread;
use std::time::Duration;

use scrap::{Capturer, Display};

pub struct CapturedFrame {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

pub struct PrimaryCapturer {
    capturer: Capturer,
    width: usize,
    height: usize,
}

impl PrimaryCapturer {
    pub fn new() -> io::Result<Self> {
        let display = Display::primary()?;
        let capturer = Capturer::new(display)?;
        let width = capturer.width();
        let height = capturer.height();

        Ok(Self {
            capturer,
            width,
            height,
        })
    }

    pub fn capture_frame(&mut self) -> io::Result<CapturedFrame> {
        loop {
            match self.capturer.frame() {
                Ok(frame) => {
                    let stride = frame.len() / self.height;
                    let mut packed = vec![0_u8; self.width * self.height * 4];

                    for row in 0..self.height {
                        let source_start = row * stride;
                        let source_end = source_start + (self.width * 4);
                        let target_start = row * self.width * 4;
                        let target_end = target_start + (self.width * 4);
                        packed[target_start..target_end]
                            .copy_from_slice(&frame[source_start..source_end]);
                    }

                    return Ok(CapturedFrame {
                        width: self.width,
                        height: self.height,
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
}
