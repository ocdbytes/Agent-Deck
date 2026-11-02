# agent-deck-firmware

ESP32 firmware for the deck. It reads the session frames the Mac sends over
the USB cable and draws them on a 2.8" 240x320 ILI9341 SPI LCD with
[`deck::Deck`](../deck/). Two buttons turn the pages.

It is built from this directory with the `esp` toolchain, outside the main
workspace, like esp32-libs' firmware demos.

## Wiring

| Part | Pin | ESP32 | Notes |
| ---- | --- | ----- | ----- |
| LCD | VCC, LED | 3V3 | Power and backlight, always on |
| LCD | GND | GND | |
| LCD | SCK | GPIO23 | SPI clock |
| LCD | SDI (MOSI) | GPIO22 | Data to the panel |
| LCD | CS | GPIO5 | Chip select |
| LCD | D/C | GPIO19 | Data/command |
| LCD | RESET | GPIO18 | |
| LCD | SDO (MISO), touch pins | not connected | The deck never reads from the panel |
| Left button | one leg | GPIO32 | Previous page; diagonal leg to GND |
| Right button | one leg | GPIO33 | Next page; diagonal leg to GND |

The buttons use the internal pull-ups, so a pin reads low while its button
is pressed. The diagonal legs of a 4-pin button are never joined inside, so
they work whichever way it is turned.

Keep GPIO1 and GPIO3 free: they are UART0, which carries the session frames
over the USB cable (115200 baud). GPIO6 to GPIO11 are wired to the flash
chip, and GPIO0, 2, 12 and 15 are strapping pins. GPIO34 to GPIO39 have no
internal pull-ups, so a button there needs a 10 kΩ resistor to 3V3.

The pins are set in `src/main.rs`.

## Flash

From the repository root:

```sh
scripts/flash.sh                          # pick the port when asked
scripts/flash.sh /dev/cu.usbserial-0001   # or name it
```

The script builds this crate in release mode, flashes it with `espflash`,
then starts `agent-deck` on the Mac to send the sessions. The first time,
install the toolchain as in esp32-libs'
[setup chapter](https://github.com/ocdbytes/esp32-libs/tree/main/rust-esp32-book).

To flash and watch the ESP32's log instead, run `cargo run --release` here
(after `source ~/export-esp.sh`). The monitor holds the port, so stop it
before starting `agent-deck`.

## How it works

- **Input:** the Mac sends a frame about once a second. Each loop reads the
  UART's waiting bytes into the deck without blocking.
- **Redraws:** the deck redraws the whole panel only when a new frame
  arrives or the page changes, so the panel does not flicker and the UART is
  idle while the panel is written. A frame lost to a full UART buffer fails
  its checksum, and the next one replaces it.
- **Animations:** 30 times a second (`deck::FPS`) the deck advances its
  animations and writes only their areas, the Claude Code mascots and Codex
  pets on the page: about 3.2 KB each, short enough for the UART's 128-byte
  FIFO to hold the bytes that arrive meanwhile.
- **Paging:** each press of the left button shows the previous page and
  each press of the right one the next, with 50 ms of debounce.

## Tuning

If the picture is wrong, change the constants at the top of `src/main.rs`:

| Symptom | Constant |
| ------- | -------- |
| Picture mirrored | `ORIENTATION`: remove `.flip_horizontal()` |
| Picture upside down | `ORIENTATION`: add `.rotate(Rotation::Deg180)` before `.flip_horizontal()` |
| Red and blue swapped | `COLOR_ORDER` to `Rgb` |
| Black shows as white | `COLOR_INVERSION` to `Inverted` |
| Garbled image | Lower `SPI_FREQUENCY` |
