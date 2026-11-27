//! Shows the deck in a resizable desktop window with the sessions running on
//! this Mac. ← and → turn the pages; Esc quits.
//!
//! Run with `cargo run -p deck --example preview --features sim`.

use std::{
    env, thread,
    time::{Duration, Instant, SystemTime},
};

use deck::{Deck, FPS, SIZE, host};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics_simulator::{OutputSettingsBuilder, SimulatorDisplay};
use frames::Scene;
use link_lib::MAX_FRAME_LEN;
use sdl2::{event::Event, keyboard::Keycode, pixels::PixelFormatEnum, render::ScaleMode};

/// How often to fetch the sessions, like the Mac side does for the deck.
const INTERVAL: Duration = Duration::from_secs(1);

/// Initial window pixels per panel pixel.
const INITIAL_SCALE: u32 = 1;

/// PNG pixels per panel pixel.
const DUMP_SCALE: u32 = 3;

fn main() {
    let mut buf = [0; MAX_FRAME_LEN];
    let (mut len, mut pos) = (0, 0);
    let mut next_fetch = Instant::now();
    // Stands in for the serial port: hands out the bytes of a fresh frame
    // every `INTERVAL`.
    let read = move || {
        if Instant::now() >= next_fetch {
            next_fetch = Instant::now() + INTERVAL;
            len = host::frame(&mut buf).expect("cannot fetch sessions").len();
            pos = 0;
        }
        let byte = buf[..len].get(pos).copied()?;
        pos += 1;
        Some(byte)
    };

    // Any changing number will do to pick the Codex pets.
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    let mut deck = Deck::new(read, seed);
    let mut display = SimulatorDisplay::<Rgb565>::new(SIZE);

    // Saves one frame as a PNG without opening a window.
    if let Some(path) = env::var_os("EG_SIMULATOR_DUMP") {
        let Ok(()) = deck.frame(&mut display);
        display
            .to_rgb_output_image(&OutputSettingsBuilder::new().scale(DUMP_SCALE).build())
            .save_png(path)
            .expect("cannot save preview");
        return;
    }

    let sdl = sdl2::init().expect("cannot start SDL");
    let video = sdl.video().expect("cannot start video");
    let window = video
        .window(
            "agent-deck",
            SIZE.width * INITIAL_SCALE,
            SIZE.height * INITIAL_SCALE,
        )
        .allow_highdpi()
        .position_centered()
        .resizable()
        .build()
        .expect("cannot open preview window");
    let mut canvas = window.into_canvas().build().expect("cannot create canvas");
    canvas
        .set_logical_size(SIZE.width, SIZE.height)
        .expect("cannot scale preview");
    // Whole multiples only, so every panel pixel is the same size.
    canvas
        .set_integer_scale(true)
        .expect("cannot scale preview");
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_streaming(PixelFormatEnum::RGB24, SIZE.width, SIZE.height)
        .expect("cannot create preview texture");
    texture.set_scale_mode(ScaleMode::Nearest);
    let mut events = sdl.event_pump().expect("cannot read window events");
    let frame_time = Duration::from_secs_f64(1.0 / f64::from(FPS));

    loop {
        let frame_start = Instant::now();
        let Ok(()) = deck.frame(&mut display);
        let image = display.to_rgb_output_image(&Default::default());

        texture
            .update(
                None,
                image.as_image_buffer().as_raw(),
                SIZE.width as usize * 3,
            )
            .expect("cannot update preview");
        canvas.clear();
        canvas
            .copy(&texture, None, None)
            .expect("cannot draw preview");
        canvas.present();

        for event in events.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => return,
                Event::KeyDown {
                    keycode: Some(Keycode::Left),
                    ..
                } => deck.prev_page(),
                Event::KeyDown {
                    keycode: Some(Keycode::Right),
                    ..
                } => deck.next_page(),
                _ => {}
            }
        }

        thread::sleep(frame_time.saturating_sub(frame_start.elapsed()));
    }
}
