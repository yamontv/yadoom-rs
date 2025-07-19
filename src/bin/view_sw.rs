use minifb::{Key, KeyRepeat, Window, WindowOptions};
use std::time::{Duration, Instant};

use yadoom_rs::{
    renderer::{Renderer, Software},
    sim::player_input,
    sim::{Angle, InputCmd, Position, TicRunner},
    wad::{Wad, load_level},
    world::{Camera, SubsectorId, TextureBank},
};

const W: usize = 1280;
const H: usize = 800;
const PLAYER_HEIGHT: f32 = 41.0;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let wad_path = args.next().expect("usage: view_sw <doom.wad>");
    let map_idx: usize = args.next().unwrap_or_else(|| "0".into()).parse()?;
    let wad = Wad::from_file(&wad_path)?;

    let mut texture_bank = TextureBank::default_with_checker();
    let mut level = load_level(&wad, wad.level_indices()[map_idx], &mut texture_bank)?;
    level.finalise_bsp();

    let mut sim = TicRunner::new(&level);

    for thing in &level.things {
        if let Some(info) = yadoom_rs::defs::by_doomednum(thing.type_id) {
            sim.spawn_mobj(
                &level,
                info,
                thing.pos.x,
                thing.pos.y,
                thing.angle,
                thing.sub_sector,
            );
        }
    }

    println!("Doom level: {}", level.name);

    let player_thing = level
        .things
        .iter()
        .find(|t| t.type_id == 1)
        .expect("no player start in map");

    let player_ent = sim.spawn_mobj(
        &level,
        yadoom_rs::defs::by_id("PLAYER").unwrap(),
        player_thing.pos.x,
        player_thing.pos.y,
        player_thing.angle,
        player_thing.sub_sector,
    );

    let mut camera = Camera::new(
        player_thing.pos.extend(41.0),
        player_thing.angle,
        90_f32.to_radians(),
    );

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

        /* --------------- build one InputCmd per tic ----------------------- */
        let mut cmd = InputCmd::default();

        /* movement --------------------------------------------------------- */
        if win.is_key_down(Key::Up) || win.is_key_down(Key::W) {
            cmd.forward += 1.0;
        }
        if win.is_key_down(Key::Down) || win.is_key_down(Key::S) {
            cmd.forward -= 1.0;
        }

        let alt = win.is_key_down(Key::LeftAlt) || win.is_key_down(Key::RightAlt);
        if alt {
            /* Alt + ←/→  = strafe */
            if win.is_key_down(Key::Left) {
                cmd.strafe -= 1.0;
            }
            if win.is_key_down(Key::Right) {
                cmd.strafe += 1.0;
            }
        } else {
            /* plain ←/→   = turn   */
            if win.is_key_down(Key::Left) {
                cmd.turn += 1.0;
            }
            if win.is_key_down(Key::Right) {
                cmd.turn -= 1.0;
            }
        }

        /* WASD strafing mirrors arrow-key strafing */
        if win.is_key_down(Key::A) {
            cmd.strafe -= 1.0;
        }
        if win.is_key_down(Key::D) {
            cmd.strafe += 1.0;
        }

        /* modifiers & actions --------------------------------------------- */
        cmd.run = win.is_key_down(Key::LeftShift) || win.is_key_down(Key::RightShift);
        cmd.fire = win.is_key_down(Key::LeftCtrl) || win.is_key_down(Key::RightCtrl);
        cmd.use_act = win.is_key_pressed(Key::Space, KeyRepeat::No); // edge-trigger

        const NUMBER_KEYS: [Key; 7] = [
            Key::Key1,
            Key::Key2,
            Key::Key3,
            Key::Key4,
            Key::Key5,
            Key::Key6,
            Key::Key7,
        ];

        for (i, &key) in NUMBER_KEYS.iter().enumerate() {
            if win.is_key_pressed(key, KeyRepeat::No) {
                cmd.weapon = Some((i + 1) as u8);
                break;
            }
        }

        /* send to ECS ------------------------------------------------------ */
        player_input(sim.world_mut(), player_ent, cmd);

        sim.pump(&level);

        if let Ok(mut q) = sim.world().query_one::<(&Position, &Angle)>(player_ent) {
            if let Some((pos, ang)) = q.get() {
                camera.pos.x = pos.0.x;
                camera.pos.y = pos.0.y;
                camera.pos.z = pos.1 + PLAYER_HEIGHT;
                camera.yaw = ang.0;
            }
        }

        // dbg!(camera);

        /* draw */
        renderer.begin_frame(W, H);
        level.fill_active_subsectors(&camera, &mut active_subsectors);
        renderer.draw_level(&active_subsectors, &level, &sim, &camera, &texture_bank);
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
