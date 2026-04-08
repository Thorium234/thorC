# ThorC

ThorC is a minimal remote desktop MVP written in Rust. It provides direct TCP connection, live screen sharing, basic mouse and keyboard control, and a simple `egui` desktop interface.

## Clone The Repository

```bash
git clone https://github.com/Thorium234/thorC.git
cd thorC
```

If you already have Git configured, that is enough to get the project locally.

## Requirements

- Rust stable toolchain
- Cargo
- A graphical Linux desktop session
- Screen capture and input permissions on the target machine
- Linux development libraries required by `scrap` and `enigo`

Install Rust if needed:

```bash
curl https://sh.rustup.rs -sSf | sh
source "$HOME/.cargo/env"
rustc --version
cargo --version
```

On Ubuntu or Debian, install the native libraries before running the app:

```bash
sudo apt update
sudo apt install -y pkg-config libx11-dev libxtst-dev libxcb1-dev libxcb-randr0-dev libxcb-shm0-dev libxdo-dev
```

If `cargo run` fails at link time with errors like `unable to find library -lxcb-randr` or `unable to find library -lxdo`, those packages are the fix.

## Build And Check

```bash
cargo fmt
cargo check
```

Run a normal debug build:

```bash
cargo run
```

## Run The GUI

Start the desktop app:

```bash
cargo run
```

ThorC is GUI-driven. In the window:

- Use `Listen` plus `Start Server` on the machine being controlled.
- Use `Target` plus `Connect` on the controller machine.
- Use `Disconnect` to close the active session.
- ThorC remembers the last listen and target addresses between launches.

## Local Test Flow

Open two terminals in the project directory.

In terminal 1:

```bash
cargo run
```

In terminal 2:

```bash
cargo run
```

Then:

- In the first window, leave `Listen` as `0.0.0.0:9000` and click `Start Server`.
- In the second window, set `Target` to `127.0.0.1:9000` and click `Connect`.

## LAN Test Flow

On the machine being controlled:

```bash
cargo run
```

On the controller machine:

```bash
cargo run
```

Then:

- On the controlled machine, click `Start Server`.
- On the controller machine, enter the server machine LAN IP, for example `192.168.1.50:9000`, then click `Connect`.

## Usage Notes

- The server machine captures and streams its screen.
- The client machine displays the streamed screen and forwards input events.
- The app is intentionally minimal and does not yet include authentication, encryption, NAT traversal, file transfer, or audio.

## Linux Notes

- `scrap` screen capture and `enigo` input simulation work best in an X11 session.
- On Wayland, screen capture or input injection may be blocked or require extra desktop portal permissions.
- If capture fails under Wayland, try logging into an X11 session and rerun the app.

## Helpful Commands

Format and check:

```bash
cargo fmt
cargo check
```
