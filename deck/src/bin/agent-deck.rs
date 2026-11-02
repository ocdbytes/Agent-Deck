//! Sends the agent sessions running on this Mac to the deck over the USB
//! cable, once a second, until stopped.
//!
//! Usage: `agent-deck [PORT]`. Without `PORT`, it uses the USB serial port
//! it finds, if there is exactly one.

use std::{env, error::Error, fs, time::Duration};

/// Speed of the USB serial cable; the firmware's `BAUD` is the same.
const BAUD: u32 = 115_200;

/// How often to send the sessions.
const INTERVAL: Duration = Duration::from_secs(1);

/// Name prefixes, under `/dev`, of the USB serial ports ESP32 boards show up
/// as: CP210x, CH340 and native USB on macOS, then USB serial on Linux.
const PORT_PREFIXES: [&str; 6] = [
    "cu.usbserial",
    "cu.SLAB_USBtoUART",
    "cu.wchusbserial",
    "cu.usbmodem",
    "ttyUSB",
    "ttyACM",
];

fn main() -> Result<(), Box<dyn Error>> {
    let path = match env::args().nth(1) {
        Some(path) => path,
        None => find_port()?,
    };
    let mut port = serialport::new(&path, BAUD)
        .timeout(Duration::from_secs(1))
        .open()
        .map_err(|err| format!("cannot open {path}: {err}"))?;
    // Release DTR and RTS so the board's auto-reset circuit lets the ESP32
    // run instead of holding it in reset or in its bootloader.
    port.write_data_terminal_ready(false)?;
    port.write_request_to_send(false)?;
    eprintln!("agent-deck: sending sessions to {path} every {INTERVAL:?}; Ctrl-C to stop");
    match deck::host::run(port, INTERVAL)? {}
}

/// Returns the one USB serial port connected, or an error saying what was
/// found.
fn find_port() -> Result<String, Box<dyn Error>> {
    let mut ports: Vec<String> = fs::read_dir("/dev")?
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| PORT_PREFIXES.iter().any(|prefix| name.starts_with(prefix)))
        .map(|name| format!("/dev/{name}"))
        .collect();
    ports.sort();
    match ports.as_slice() {
        [port] => Ok(port.clone()),
        [] => Err("no USB serial port found; is the deck plugged in?".into()),
        _ => Err(format!(
            "several USB serial ports found; pass one of: {}",
            ports.join(", ")
        )
        .into()),
    }
}
