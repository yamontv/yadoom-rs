//! BSP + minimal portal clipping (solid-colour walls).
//!
//! Controls  W/S = forward/back A/D = strafe ←/→ = turn Esc = quit
//!
//! ```bash
//! cargo run --release --bin minimal_3d -- assets/doom.wad 1
//! ```

use glam::{Vec2, Vec3, Vec3Swizzles, vec2};
use minifb::{Key, Window, WindowOptions};
use std::error::Error;
use wad::Wad;
use wad::level::*; // Node, Subsector, Seg, …

// ─── constants ──────────────────────────────────────────────────────────────
const WIDTH: usize = 1280;
const HEIGHT: usize = 800;
const HFOV: f32 = std::f32::consts::FRAC_PI_2; // 90 °
const NEAR: f32 = 0.1;
const NO_SIDE: u16 = 0xFFFF; // “no sidedef” marker

// ─── data types ─────────────────────────────────────────────────────────────
struct Camera {
    pos: Vec3,
    angle: f32,
    fov: f32,
}

struct Wall {
    v1: Vec2,
    v2: Vec2,
    floor: f32,
    ceil: f32,
    colour: u32,
    solid: bool, // one-sided wall blocks farther columns
}

// ─── entry point ────────────────────────────────────────────────────────────
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let wad_path = args.next().expect("usage: minimal_3d <doom.wad> [map]");
    let map_idx: usize = args.next().unwrap_or_else(|| "0".into()).parse()?;

    // load level
    let wad = Wad::from_file(&wad_path)?;
    let level = wad.parse_level(wad.level_indices()[map_idx])?;

    // player start (thing type 1) or origin
    let (px, py, pa) = level
        .things
        .iter()
        .find(|t| t.type_ == 1)
        .map(|t| (t.x as f32, t.y as f32, (t.angle as f32).to_radians()))
        .unwrap_or((0.0, 0.0, 0.0));

    let mut cam = Camera {
        pos: Vec3::new(px, py, 41.0),
        angle: pa,
        fov: HFOV,
    };

    // cache vertices
    let verts: Vec<Vec2> = level
        .vertices
        .iter()
        .map(|v| vec2(v.x as f32, v.y as f32))
        .collect();

    // build Wall list for every SEG (skip malformed indices)
    let seg_walls: Vec<Wall> = level
        .segs
        .iter()
        .enumerate()
        .filter_map(|(i, seg)| {
            let (v1i, v2i) = (seg.v1 as usize, seg.v2 as usize);
            if v1i >= verts.len() || v2i >= verts.len() {
                return None;
            }

            let ld = &level.linedefs[seg.linedef as usize];

            // one-sided if either sidedef index is 0xFFFF
            let solid = ld.left == NO_SIDE || ld.right == NO_SIDE;

            // choose sidedef facing the viewer for height lookup
            let side_idx = if seg.dir == 0 { ld.right } else { ld.left };
            let (fl, cl) = if side_idx == NO_SIDE {
                (0.0, 128.0)
            } else {
                level
                    .sidedefs
                    .get(side_idx as usize)
                    .and_then(|sd| level.sectors.get(sd.sector as usize))
                    .map(|s| (s.floor as f32, s.ceil as f32))
                    .unwrap_or((0.0, 128.0))
            };

            Some(Wall {
                v1: verts[v1i],
                v2: verts[v2i],
                floor: fl,
                ceil: cl,
                colour: if i & 1 == 0 { 0x00_FF0000 } else { 0x00_00FF00 },
                solid,
            })
        })
        .collect();

    // frame & occlusion buffers
    let mut frame = vec![0u32; WIDTH * HEIGHT];
    let mut solidx = vec![false; WIDTH]; // column-covered flags

    // window
    let mut win = Window::new(
        "Doom 3-D — BSP+portal clip",
        WIDTH,
        HEIGHT,
        WindowOptions::default(),
    )?;
    win.set_target_fps(70);

    // movement parameters
    let speed = 100.0;
    let rot = std::f32::consts::PI;
    let dt = 1.0 / 35.0;

    let root = level.nodes.len() - 1; // Doom root = last NODE

    // ── main loop ───────────────────────────────────────────────────────────
    while win.is_open() && !win.is_key_down(Key::Escape) {
        handle_input(&mut cam, &win, speed, rot, dt);

        frame.fill(0x00_202020);
        solidx.fill(false);

        render_child(
            Child::Node(root as u16),
            &cam,
            &level,
            &seg_walls,
            &mut frame,
            &mut solidx,
        );

        win.update_with_buffer(&frame, WIDTH, HEIGHT)?;
    }
    Ok(())
}

// ─── BSP traversal ──────────────────────────────────────────────────────────
enum Child {
    Node(u16),
    Subsector(u16),
}
impl From<u16> for Child {
    fn from(raw: u16) -> Self {
        if raw & 0x8000 != 0 {
            Child::Subsector(raw & 0x7FFF)
        } else {
            Child::Node(raw)
        }
    }
}

fn render_child(
    child: Child,
    cam: &Camera,
    lvl: &Level,
    segs: &[Wall],
    frame: &mut [u32],
    solidx: &mut [bool],
) {
    match child {
        Child::Subsector(ss) => {
            if let Some(ssec) = lvl.subsectors.get(ss as usize) {
                for i in 0..ssec.seg_count {
                    let id = ssec.first_seg as usize + i as usize;
                    if let Some(w) = segs.get(id) {
                        draw_wall(cam, w, frame, solidx);
                    }
                }
            }
        }
        Child::Node(idx) => {
            if let Some(node) = lvl.nodes.get(idx as usize) {
                let side = point_side(cam.pos.xy(), node) as usize;
                render_child(Child::from(node.child[side]), cam, lvl, segs, frame, solidx);
                render_child(
                    Child::from(node.child[side ^ 1]),
                    cam,
                    lvl,
                    segs,
                    frame,
                    solidx,
                );
            }
        }
    }
}

// 0 = front, 1 = back
fn point_side(p: Vec2, n: &Node) -> i32 {
    let d = (p.x - n.x as f32) * n.dy as f32 - (p.y - n.y as f32) * n.dx as f32;
    if d >= 0.0 { 0 } else { 1 }
}

// ─── wall projection with per-column occlusion ──────────────────────────────
fn draw_wall(cam: &Camera, w: &Wall, buf: &mut [u32], solidx: &mut [bool]) {
    let half_w = WIDTH as f32 * 0.5;
    let half_h = HEIGHT as f32 * 0.5;
    let focal = half_w / (cam.fov * 0.5).tan();

    // world → camera
    let rel1 = w.v1 - cam.pos.xy();
    let rel2 = w.v2 - cam.pos.xy();
    let (sin, cos) = cam.angle.sin_cos();
    let p1 = Vec2::new(rel1.x * cos + rel1.y * sin, -rel1.x * sin + rel1.y * cos);
    let p2 = Vec2::new(rel2.x * cos + rel2.y * sin, -rel2.x * sin + rel2.y * cos);

    if p1.y <= NEAR && p2.y <= NEAR {
        return;
    }
    let (p1, p2) = clip_near(p1, p2);

    // screen X span
    let sx1 = half_w + p1.x * focal / p1.y;
    let sx2 = half_w + p2.x * focal / p2.y;
    let (ix1, ix2) = {
        let (mut a, mut b) = (sx1 as i32, sx2 as i32);
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        (a.clamp(0, WIDTH as i32 - 1), b.clamp(0, WIDTH as i32 - 1))
    };
    if ix1 == ix2 {
        return;
    }

    // vertical spans
    let eye = cam.pos.z;
    let top1 = half_h - (w.ceil - eye) * focal / p1.y;
    let bot1 = half_h - (w.floor - eye) * focal / p1.y;
    let top2 = half_h - (w.ceil - eye) * focal / p2.y;
    let bot2 = half_h - (w.floor - eye) * focal / p2.y;

    let col = w.colour;

    for x in ix1..=ix2 {
        if solidx[x as usize] {
            continue;
        } // fully occluded

        let t = (x as f32 - sx1) / (sx2 - sx1);
        let top = top1 + (top2 - top1) * t;
        let bot = bot1 + (bot2 - bot1) * t;
        let (ya, yb) = (top as i32, bot as i32);
        if ya >= HEIGHT as i32 || yb < 0 || ya >= yb {
            continue;
        }
        let (y0, y1) = (
            ya.clamp(0, HEIGHT as i32 - 1),
            yb.clamp(0, HEIGHT as i32 - 1),
        );

        for y in y0..=y1 {
            buf[y as usize * WIDTH + x as usize] = col;
        }

        if w.solid {
            solidx[x as usize] = true;
        }
    }
}

// near-plane clip
fn clip_near(mut p1: Vec2, mut p2: Vec2) -> (Vec2, Vec2) {
    if p1.y < NEAR {
        let t = (NEAR - p1.y) / (p2.y - p1.y);
        p1 = Vec2::new(p1.x + (p2.x - p1.x) * t, NEAR);
    }
    if p2.y < NEAR {
        let t = (NEAR - p2.y) / (p1.y - p2.y);
        p2 = Vec2::new(p2.x + (p1.x - p2.x) * t, NEAR);
    }
    (p1, p2)
}

// ─── input handling ─────────────────────────────────────────────────────────
fn handle_input(cam: &mut Camera, win: &Window, speed: f32, rot: f32, dt: f32) {
    if win.is_key_down(Key::Left) {
        cam.angle += rot * dt;
    }
    if win.is_key_down(Key::Right) {
        cam.angle -= rot * dt;
    }

    let (sin, cos) = cam.angle.sin_cos();
    let mut dx = 0.0;
    let mut dy = 0.0;
    if win.is_key_down(Key::W) || win.is_key_down(Key::Up) {
        dy += speed * dt;
    }
    if win.is_key_down(Key::S) || win.is_key_down(Key::Down) {
        dy -= speed * dt;
    }
    if win.is_key_down(Key::A) {
        dx -= speed * dt;
    }
    if win.is_key_down(Key::D) {
        dx += speed * dt;
    }

    cam.pos.x -= cos * dx + sin * dy;
    cam.pos.y -= sin * dx - cos * dy;
}
