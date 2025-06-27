use minifb::{Key, Window, WindowOptions};
use std::time::{Duration, Instant};
use yadoom_rs::{
    renderer::{Renderer, software::Software},
    wad::{loader, raw::Wad},
    world::{camera::Camera, geometry::SubsectorId, texture::TextureBank},
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

    let mut texture_bank = TextureBank::default_with_checker();
    let mut level = loader::load_level(&wad, wad.level_indices()[map_idx], &mut texture_bank)?;
    level.finalise_bsp();

    let player = level.things.iter().find(|t| t.type_id == 1).unwrap();
    let mut camera = Camera::new(player.pos.extend(41.0), player.angle, 90_f32.to_radians());
    // let camera = Camera::new(
    //     glam::Vec3::new(2933.7625, -2822.0237, 41.0),
    //     5.0714335,
    //     90_f32.to_radians(),
    // );

    let mut renderer = Software::default();

    let mut win = Window::new("Rust Doom Software Render", W, H, WindowOptions::default())?;
    win.set_target_fps(35);

    // ────────────────── benchmarking state ──────────────────────────────
    let mut acc_time = Duration::ZERO; // cumulated render time
    let mut acc_frames = 0usize; // frames in the current window
    let mut last_print = Instant::now(); // when we printed last

    let mut active_subsectors: Vec<SubsectorId> = Vec::new();

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

        camera.turn(yaw);
        camera.step(dy, dx);

        // dbg!(engine.camera);

        /* draw */
        renderer.begin_frame(W, H);
        level.fill_active_subsectors(&camera, &mut active_subsectors);
        renderer.draw_subsectors(&active_subsectors, &level, &camera, &texture_bank, &mut win);
        renderer.end_frame(|fb, w, h| {
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
