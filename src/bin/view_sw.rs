use minifb::{Key, Window, WindowOptions};
use std::time::{Duration, Instant};
use yadoom_rs::{
    engine::pipeline,
    renderer::software::Software,
    wad::{Wad, loader},
    world::{camera::Camera, texture::TextureBank},
};

const W: usize = 1280;
const H: usize = 800;
const SPEED: f32 = 150.0;
const TURN: f32 = std::f32::consts::PI;
const DT: f32 = 1. / 35.;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let wad_path = args.next().expect("usage: view_sw <doom.wad>");
    let map_idx: usize = args.next().unwrap_or_else(|| "0".into()).parse()?;
    let wad = Wad::from_file(&wad_path)?;

    let mut bank = TextureBank::default_with_checker();
    let mut level = loader::load_level(&wad, wad.level_indices()[map_idx], &mut bank)?;
    level.finalise_bsp();

    let player = level.things.iter().find(|t| t.type_id == 1).unwrap();
    let mut cam = Camera::new(player.pos.extend(41.0), player.angle, 110_f32.to_radians());

    let mut win = Window::new("Software Doom viewer", W, H, WindowOptions::default())?;
    win.set_target_fps(35);
    let mut sw = Software::default();

    // ────────────────── benchmarking state ──────────────────────────────
    let mut acc_time = Duration::ZERO; // cumulated render time
    let mut acc_frames = 0usize; // frames in the current window
    let mut last_print = Instant::now(); // when we printed last

    while win.is_open() && !win.is_key_down(Key::Escape) {
        let t0 = Instant::now(); // ┌─ frame timer start

        /* movement intent */
        let (mut dx, mut dy, mut yaw) = (0., 0., 0.);
        if win.is_key_down(Key::W) || win.is_key_down(Key::Up) {
            dy += SPEED * DT;
        }
        if win.is_key_down(Key::S) || win.is_key_down(Key::Down) {
            dy -= SPEED * DT;
        }
        if win.is_key_down(Key::A) {
            dx += SPEED * DT;
        }
        if win.is_key_down(Key::D) {
            dx -= SPEED * DT;
        }
        if win.is_key_down(Key::Left) {
            yaw += TURN * DT;
        }
        if win.is_key_down(Key::Right) {
            yaw -= TURN * DT;
        }

        cam.turn(yaw);
        cam.step(dy, dx);

        /* draw */
        pipeline::render_frame(&mut sw, &level, &cam, &bank, W, H, |fb, w, h| {
            // ─────────── accumulate & report every ~3 s ────────────────────
            acc_time += t0.elapsed();
            acc_frames += 1;
            win.update_with_buffer(fb, w, h).unwrap()
        });

        if last_print.elapsed() >= Duration::from_secs(3) {
            let avg_ms = acc_time.as_secs_f64() * 1000.0 / acc_frames as f64;
            let fps = 1000.0 / avg_ms;
            println!("avg render: {:.2} ms  ({:.1} FPS)", avg_ms, fps);
            acc_time = Duration::ZERO;
            acc_frames = 0;
            last_print = Instant::now();
        }
    }
    Ok(())
}
