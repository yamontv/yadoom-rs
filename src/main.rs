//! Minimal 2-D Doom map viewer.
//!
//! ```bash
//! cargo run --release -- <doom.wad> [map_idx]
//! ```

use minifb::{Key, Window, WindowOptions};
use std::error::Error;

use yadoom_rs::wad::Wad;

const WIDTH: usize = 1024;
const HEIGHT: usize = 768;

fn main() -> Result<(), Box<dyn Error>> {
    // ─────────── parse CLI ────────────
    let mut args = std::env::args().skip(1);
    let wad_path = args.next().expect("usage: <prog> <doom.wad> [map_idx]");
    let map_idx: usize = args
        .next()
        .unwrap_or_else(|| "0".into())
        .parse()
        .expect("map_idx should be a number");

    // ─────────── load WAD & map ───────
    let wad = Wad::from_file(&wad_path)?;
    let markers = wad.level_indices();
    if map_idx >= markers.len() {
        eprintln!("map_idx {map_idx} out of range ({} maps)", markers.len());
        std::process::exit(1);
    }
    let level = wad.parse_level(markers[map_idx])?;

    println!("{}", Wad::lump_name(&wad.lumps[markers[map_idx]].name));

    // ─────────── map‑space → screen‑space transform ────────────
    let (min_x, max_x) = level
        .vertices
        .iter()
        .fold((i16::MAX, i16::MIN), |(lo, hi), v| {
            (lo.min(v.x), hi.max(v.x))
        });
    let (min_y, max_y) = level
        .vertices
        .iter()
        .fold((i16::MAX, i16::MIN), |(lo, hi), v| {
            (lo.min(v.y), hi.max(v.y))
        });

    let map_w = (max_x - min_x) as f32;
    let map_h = (max_y - min_y) as f32;
    let scale = (WIDTH as f32 / map_w).min(HEIGHT as f32 / map_h) * 0.9; // 10 % margin
    let offset_x = (WIDTH as f32 - map_w * scale) / 2.0;
    let offset_y = (HEIGHT as f32 - map_h * scale) / 2.0;

    let to_screen = |vx: i16, vy: i16| -> (i32, i32) {
        let sx = ((vx - min_x) as f32 * scale + offset_x) as i32;
        let sy = HEIGHT as i32 - ((vy - min_y) as f32 * scale + offset_y) as i32; // invert Y so north is up
        (sx, sy)
    };

    // ─────────── rasterise linedefs ────────────
    let mut buffer = vec![0u32; WIDTH * HEIGHT];
    for ld in &level.linedefs {
        // Borrow vertices instead of moving them out of the Vec
        let v1 = &level.vertices[ld.v1 as usize];
        let v2 = &level.vertices[ld.v2 as usize];
        let (x0, y0) = to_screen(v1.x, v1.y);
        let (x1, y1) = to_screen(v2.x, v2.y);
        draw_line(&mut buffer, WIDTH, HEIGHT, x0, y0, x1, y1, 0x00_FFFFFF);
    }

    // ─────────── show window ────────────
    let mut window = Window::new("Doom map", WIDTH, HEIGHT, WindowOptions::default())?;
    while window.is_open() && !window.is_key_down(Key::Escape) {
        window.update_with_buffer(&buffer, WIDTH, HEIGHT)?;
    }
    Ok(())
}

/// Integer Bresenham line‑drawing algorithm.
fn draw_line(
    buf: &mut [u32],
    w: usize,
    h: usize,
    mut x0: i32,
    mut y0: i32,
    x1: i32,
    y1: i32,
    colour: u32,
) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        if (0..w as i32).contains(&x0) && (0..h as i32).contains(&y0) {
            buf[y0 as usize * w + x0 as usize] = colour;
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            if x0 == x1 {
                break;
            }
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            if y0 == y1 {
                break;
            }
            err += dx;
            y0 += sy;
        }
    }
}
