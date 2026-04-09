use std::io;

use enigo::{Enigo, MouseButton, MouseControllable};

pub fn execute_mouse_event(x: i32, y: i32, button: &str) -> io::Result<()> {
    let mut enigo = Enigo::new();
    enigo.mouse_move_to(x, y);

    match button {
        "left" => enigo.mouse_click(MouseButton::Left),
        "right" => enigo.mouse_click(MouseButton::Right),
        "middle" => enigo.mouse_click(MouseButton::Middle),
        "move" | "" => {}
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported mouse button: {other}"),
            ))
        }
    }

    Ok(())
}

pub fn execute_mouse_scroll(x: i32, y: i32, delta_x: i32, delta_y: i32) -> io::Result<()> {
    let mut enigo = Enigo::new();
    enigo.mouse_move_to(x, y);

    if delta_x != 0 {
        enigo.mouse_scroll_x(delta_x);
    }

    if delta_y != 0 {
        enigo.mouse_scroll_y(delta_y);
    }

    Ok(())
}
