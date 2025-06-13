//! Minimal BSP‐debug viewer.
//!
//! ```bash
//! cargo run --release --bin bsp_view -- assets/doom.wad 0
//! ```
//!
//! Controls W/S = forward/back A/D = strafe ←/→ = turn Esc = quit

use glam::{Vec2, vec2};
use minifb::{Key, Window, WindowOptions};

use yadoom_rs::wad::{Wad, loader};
use yadoom_rs::world::{geometry::Level, texture::TextureBank};

const WIDTH: usize = 1024;
const HEIGHT: usize = 768;

/*───────────────────────── drawing helpers ─────────────────────────*/

fn to_screen(v: Vec2, min: Vec2, scale: f32, off: Vec2) -> (i32, i32) {
    (
        ((v.x - min.x) * scale + off.x).round() as i32,
        HEIGHT as i32 - ((v.y - min.y) * scale + off.y).round() as i32,
    )
}

fn draw_line(buf: &mut [u32], x0: i32, y0: i32, x1: i32, y1: i32, col: u32) {
    let mut x0 = x0;
    let mut y0 = y0;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if (0..WIDTH as i32).contains(&x0) && (0..HEIGHT as i32).contains(&y0) {
            buf[y0 as usize * WIDTH + x0 as usize] = col;
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

/*──────────────────────────── main ────────────────────────────────*/
fn main() {
    /*----- CLI ------------------------------------------------------*/
    let mut args = std::env::args().skip(1);
    let wad_path = args.next().expect("usage: bsp_view <wad> [map]");
    let map_idx: usize = args.next().unwrap_or_else(|| "0".into()).parse().unwrap();

    /*----- load map -------------------------------------------------*/
    let wad = Wad::from_file(wad_path).expect("open wad");
    let mut bank = TextureBank::default_with_checker();
    let mut lvl = loader::load_level(&wad, wad.level_indices()[map_idx], &mut bank).unwrap();
    lvl.finalise_bsp();

    /*----- viewport transform pre-calc -----------------------------*/
    let (min, max) = lvl.vertices.iter().fold(
        (vec2(f32::MAX, f32::MAX), vec2(f32::MIN, f32::MIN)),
        |(lo, hi), v| (lo.min(v.pos), hi.max(v.pos)),
    );
    let map_w = max.x - min.x;
    let map_h = max.y - min.y;
    let scale = (WIDTH as f32 / map_w).min(HEIGHT as f32 / map_h) * 0.9;
    let off = vec2(
        (WIDTH as f32 - map_w * scale) * 0.5,
        (HEIGHT as f32 - map_h * scale) * 0.5,
    );

    /*----- starting position = player 1 start ----------------------*/
    let start = lvl
        .things
        .iter()
        .find(|t| t.type_id == 1)
        .map(|t| t.pos)
        .unwrap_or_else(|| vec2(0.0, 0.0));
    let mut pos = start;
    let mut angle = 0.0_f32;

    /*----- window ---------------------------------------------------*/
    let mut buf = vec![0u32; WIDTH * HEIGHT];
    let mut win =
        Window::new("BSP viewer", WIDTH, HEIGHT, WindowOptions::default()).expect("window");
    win.set_target_fps(60);

    /*----- movement constants --------------------------------------*/
    let speed = 128.0;
    let rot = std::f32::consts::PI; // 180°/s
    let dt = 1.0 / 60.0;

    /*========================== main loop ==========================*/
    while win.is_open() && !win.is_key_down(Key::Escape) {
        /*--- input --------------------------------------------------*/
        let (sin, cos) = angle.sin_cos();
        let mut dx = 0.0;
        let mut dy = 0.0;
        if win.is_key_down(Key::W) {
            dy += speed * dt;
        }
        if win.is_key_down(Key::S) {
            dy -= speed * dt;
        }
        if win.is_key_down(Key::A) {
            dx -= speed * dt;
        }
        if win.is_key_down(Key::D) {
            dx += speed * dt;
        }
        if win.is_key_down(Key::Left) {
            angle += rot * dt;
        }
        if win.is_key_down(Key::Right) {
            angle -= rot * dt;
        }

        pos.x += cos * dy + sin * dx;
        pos.y += sin * dy - cos * dx;

        /*--- determine current leaf & sector -----------------------*/
        let ss_idx = lvl.locate_subsector(pos);
        let sector = lvl.sector_of_subsector[ss_idx as usize];

        /*--- clear --------------------------------------------------*/
        buf.fill(0x00303030);

        /*--- draw every seg (grey) ---------------------------------*/
        for seg in &lvl.segs {
            let a = lvl.vertices[seg.v1 as usize].pos;
            let b = lvl.vertices[seg.v2 as usize].pos;
            let (x0, y0) = to_screen(a, min, scale, off);
            let (x1, y1) = to_screen(b, min, scale, off);
            draw_line(&mut buf, x0, y0, x1, y1, 0x00555555);
        }

        /*--- highlight sector (red) --------------------------------*/
        for ld_idx in lvl.linedefs_of_sector(sector) {
            let ld = &lvl.linedefs[ld_idx as usize];
            let a = lvl.vertices[ld.v1 as usize].pos;
            let b = lvl.vertices[ld.v2 as usize].pos;
            let (x0, y0) = to_screen(a, min, scale, off);
            let (x1, y1) = to_screen(b, min, scale, off);
            draw_line(&mut buf, x0, y0, x1, y1, 0x00FF0000);
        }

        /*--- highlight subsector (yellow) ---------------------------*/
        for seg_idx in lvl.segs_of_subsector(ss_idx) {
            let seg = &lvl.segs[seg_idx as usize];
            let a = lvl.vertices[seg.v1 as usize].pos;
            let b = lvl.vertices[seg.v2 as usize].pos;
            let (x0, y0) = to_screen(a, min, scale, off);
            let (x1, y1) = to_screen(b, min, scale, off);
            draw_line(&mut buf, x0, y0, x1, y1, 0x00FFFF00);
        }

        /*--- draw player -------------------------------------------*/
        let (px, py) = to_screen(pos, min, scale, off);
        for dy in -2..=2 {
            for dx in -2..=2 {
                let x = px + dx;
                let y = py + dy;
                if (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y) {
                    buf[y as usize * WIDTH + x as usize] = 0x00FFFFFF;
                }
            }
        }

        win.update_with_buffer(&buf, WIDTH, HEIGHT).unwrap();
    }
}
