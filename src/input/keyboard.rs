use std::io;

use enigo::{Enigo, Key, KeyboardControllable};

pub fn execute_keyboard_event(key: &str) -> io::Result<()> {
    let mut enigo = Enigo::new();

    match key {
        "enter" => enigo.key_click(Key::Return),
        "tab" => enigo.key_click(Key::Tab),
        "backspace" => enigo.key_click(Key::Backspace),
        "escape" => enigo.key_click(Key::Escape),
        "space" => enigo.key_click(Key::Space),
        text if text.chars().count() == 1 => enigo.key_sequence(text),
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported key: {other}"),
            ))
        }
    }

    Ok(())
}
