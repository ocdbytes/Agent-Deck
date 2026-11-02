//! Agent deck firmware for an ESP32 with a 2.8" 240x320 ILI9341 SPI LCD.
//!
//! Reads the session frames the Mac sends over the USB cable (UART0) and
//! draws them with [`deck::Deck`]. Two buttons turn the pages.
//!
//! Wiring (-> ESP32): LCD VCC -> 3V3, GND -> GND, CS -> GPIO5, RESET ->
//! GPIO18, DC -> GPIO19, SDI (MOSI) -> GPIO22, SCK -> GPIO23, LED -> 3V3. SDO
//! (MISO) and the touch pins stay unconnected. Left button GPIO32 -> GND,
//! right button GPIO33 -> GND.
//!
//! Build, flash and start the Mac side with `scripts/flash.sh`.
#![no_std]
#![no_main]

use deck::Deck;
use embedded_graphics::{
    framebuffer::{Framebuffer, buffer_size},
    image::Image,
    pixelcolor::{
        Rgb565,
        raw::{BigEndian, RawU16},
    },
    prelude::*,
};
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::main;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::time::{Duration, Instant, Rate};
use esp_hal::uart::{Config as UartConfig, UartRx};
use frames::Scene;
use mipidsi::Builder;
use mipidsi::interface::SpiInterface;
use mipidsi::models::ILI9341Rgb565;
use mipidsi::options::{ColorInversion, ColorOrder, Orientation};
use static_cell::ConstStaticCell;

/// Panel width in pixels, in portrait orientation.
const WIDTH: usize = 240;

/// Panel height in pixels.
const HEIGHT: usize = 320;

/// Panel orientation. With no flags, the ILI9341 scans columns in the order
/// that mirrors the picture on common 2.8" modules, so the columns are
/// reversed (`flip_horizontal`) for an upright portrait. If the picture is
/// still mirrored, remove `.flip_horizontal()`; if it is upside down, add
/// `.rotate(Rotation::Deg180)` before it.
const ORIENTATION: Orientation = Orientation::new().flip_horizontal();

/// SPI clock. Most modules work at 40 MHz; lower it if the image is garbled.
const SPI_FREQUENCY: Rate = Rate::from_mhz(40);

/// Subpixel order. Flip to `Rgb` if red and blue are swapped.
const COLOR_ORDER: ColorOrder = ColorOrder::Bgr;

/// Color inversion. Flip to `Inverted` if black shows as white.
const COLOR_INVERSION: ColorInversion = ColorInversion::Normal;

/// Speed of the USB serial cable. The Mac side (`agent-deck`) uses the same.
const BAUD: u32 = 115_200;

/// How long a button must have been released for a press to count, so
/// contact bounce does not turn several pages.
const DEBOUNCE: Duration = Duration::from_millis(50);

/// Off-screen frame the deck draws into; sent to the panel in one write.
type Frame =
    Framebuffer<Rgb565, RawU16, BigEndian, WIDTH, HEIGHT, { buffer_size::<Rgb565>(WIDTH, HEIGHT) }>;

/// The frame is 150 KB, far too big for the stack, so it lives in static RAM.
static FRAME: ConstStaticCell<Frame> = ConstStaticCell::new(Frame::new());

/// Busy-wait delay for the display's reset sequence.
struct BusyDelay;

impl DelayNs for BusyDelay {
    fn delay_ns(&mut self, ns: u32) {
        let wait = Duration::from_micros(u64::from(ns.div_ceil(1_000)));
        let start = Instant::now();
        while start.elapsed() < wait {}
    }
}

/// A push button from a GPIO to GND, read with the internal pull-up.
struct Button<'d> {
    pin: Input<'d>,
    was_down: bool,
    up_since: Instant,
}

impl<'d> Button<'d> {
    fn new(pin: Input<'d>) -> Self {
        Self {
            pin,
            was_down: false,
            up_since: Instant::now(),
        }
    }

    /// Returns `true` once per press, ignoring contact bounce.
    fn pressed(&mut self) -> bool {
        // The pull-up holds the pin high; the button pulls it low.
        let down = self.pin.is_low();
        let pressed = down && !self.was_down && self.up_since.elapsed() >= DEBOUNCE;
        if !down && self.was_down {
            self.up_since = Instant::now();
        }
        self.was_down = down;
        pressed
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log::error!("{info}");
    loop {}
}

esp_bootloader_esp_idf::esp_app_desc!();

#[main]
fn main() -> ! {
    esp_println::logger::init_logger_from_env();
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default().with_frequency(SPI_FREQUENCY),
    )
    .unwrap()
    .with_sck(peripherals.GPIO23)
    .with_mosi(peripherals.GPIO22);
    let cs = Output::new(peripherals.GPIO5, Level::High, OutputConfig::default());
    let dc = Output::new(peripherals.GPIO19, Level::Low, OutputConfig::default());
    let rst = Output::new(peripherals.GPIO18, Level::High, OutputConfig::default());
    let spi = ExclusiveDevice::new_no_delay(spi, cs).unwrap();
    let mut buffer = [0u8; 512];
    let interface = SpiInterface::new(spi, dc, &mut buffer);
    let mut display = Builder::new(ILI9341Rgb565, interface)
        .reset_pin(rst)
        .orientation(ORIENTATION)
        .color_order(COLOR_ORDER)
        .invert_colors(COLOR_INVERSION)
        .init(&mut BusyDelay)
        .unwrap();

    // The USB cable's serial chip is wired to UART0, RX on GPIO3.
    let mut rx = UartRx::new(peripherals.UART0, UartConfig::default().with_baudrate(BAUD))
        .unwrap()
        .with_rx(peripherals.GPIO3);
    let mut deck = Deck::new(move || {
        let mut byte = [0];
        // An overflow loses bytes; the frame's checksum then drops the frame.
        matches!(rx.read_buffered(&mut byte), Ok(1)).then_some(byte[0])
    });
    let pull_up = InputConfig::default().with_pull(Pull::Up);
    let mut left = Button::new(Input::new(peripherals.GPIO32, pull_up));
    let mut right = Button::new(Input::new(peripherals.GPIO33, pull_up));
    log::info!("agent deck ready, waiting for frames at {BAUD} baud");

    let frame = FRAME.take();
    let mut redraw = true;
    loop {
        redraw |= deck.receive();
        if left.pressed() {
            deck.prev_page();
            redraw = true;
        }
        if right.pressed() {
            deck.next_page();
            redraw = true;
        }

        // Redraw only on changes: the Mac sends about one frame a second, so
        // the UART is idle while the panel is written.
        if redraw {
            redraw = false;
            let Ok(()) = frame.clear(Rgb565::BLACK);
            let Ok(()) = deck.draw(frame);
            if let Err(e) = Image::new(&frame.as_image(), Point::zero()).draw(&mut display) {
                log::warn!("display write failed: {e:?}");
            }
        }
    }
}
