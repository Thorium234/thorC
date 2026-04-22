# ThorC

ThorC is a simple Rust remote desktop tool for local or LAN use. It does one job: connect directly to another machine over TCP, stream the screen, and forward basic mouse and keyboard input through a small `egui` desktop app.

## What It Does

- Direct TCP host/client connection
- Live screen viewer
- Basic mouse movement, click, scroll, and keyboard input
- Remembered listen and target addresses between launches

## What It Does Not Do

- Authentication
- Encryption
- NAT traversal or relay support
- File transfer
- Audio

## Requirements

- Rust stable toolchain
- Cargo
- A graphical Linux desktop session
- Screen capture and input permissions on the host machine
- Linux development libraries required by `scrap` and `enigo`

Install Rust if needed:

```bash
curl https://sh.rustup.rs -sSf | sh
source "$HOME/.cargo/env"
rustc --version
cargo --version
```

On Ubuntu or Debian, install the native libraries before building:

```bash
sudo apt update
sudo apt install -y pkg-config libx11-dev libxtst-dev libxcb1-dev libxcb-randr0-dev libxcb-shm0-dev libxdo-dev
```

If `cargo run` fails at link time with errors like `unable to find library -lxcb-randr` or `unable to find library -lxdo`, those packages are the fix.

## Run

```bash
cargo run
```

ThorC is GUI-driven:

- On the machine being controlled, use `Start Server` with a listen address like `0.0.0.0:9000`.
- On the controller machine, use `Connect` with a target like `127.0.0.1:9000` or `192.168.1.50:9000`.
- Use `Disconnect` to end the active session.

## Local Test

Open two terminals in the project directory and run:

```bash
cargo run
```

in both.

Then:

- In the first window, leave the listen address as `0.0.0.0:9000` and click `Start Server`.
- In the second window, set the target to `127.0.0.1:9000` and click `Connect`.

## LAN Test

- Start ThorC on the machine being controlled and click `Start Server`.
- Start ThorC on the controller machine.
- Enter the host machine LAN IP and port, for example `192.168.1.50:9000`, then click `Connect`.

## Build And Verify

```bash
cargo fmt
cargo check
cargo test
```

## Linux Notes

- `scrap` screen capture and `enigo` input simulation work best in an X11 session.
- On Wayland, screen capture or input injection may be blocked or require extra portal permissions.
- If capture fails under Wayland, try an X11 session.
