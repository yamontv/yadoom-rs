//! Textured walls (full TEXTURE1/2 + PNAMES) with perspective-correct
//! sampling. Blank (“-”) textures are skipped. BSP order + portal clipping.
//!
//! Controls  W/S forward · A/D strafe · ←/→ turn · Esc quit
//!
//! Build:  cargo run --release --bin textures_3d -- assets/doom.wad 1

use glam::{Vec2, Vec3, Vec3Swizzles, vec2};
use minifb::{Key, Window, WindowOptions};
use std::{
    collections::HashMap,
    error::Error,
    io::{Cursor, Read},
};

use yadoom_rs::wad::{Wad, level::*};

// ─── constants ──────────────────────────────────────────────────────────────
const WIDTH: usize = 640;
const HEIGHT: usize = 400;
const HFOV: f32 = 110.0_f32.to_radians();
const NEAR: f32 = 1.0;
const NO_SIDE: u16 = 0xFFFF;

// ─── helper fns ─────────────────────────────────────────────────────────────
fn norm(name: &str) -> String {
    name.trim_matches(['\0', ' '].as_ref()).to_ascii_uppercase()
}
fn lump_name(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).unwrap_or("MISSING")
}
fn str_from(b: &[u8; 8]) -> &str {
    let end = b.iter().position(|&c| c == 0).unwrap_or(8);
    std::str::from_utf8(&b[..end]).unwrap_or("MISSING")
}
fn read_i16(c: &mut Cursor<&[u8]>) -> i16 {
    let mut b = [0u8; 2];
    c.read_exact(&mut b).unwrap();
    i16::from_le_bytes(b)
}
fn read_i32(c: &mut Cursor<&[u8]>) -> i32 {
    let mut b = [0u8; 4];
    c.read_exact(&mut b).unwrap();
    i32::from_le_bytes(b)
}
fn read_name<'a>(c: &mut Cursor<&'a [u8]>) -> &'a [u8] {
    let pos = c.position() as usize;
    c.set_position((pos + 8) as u64);
    &c.get_ref()[pos..pos + 8]
}

// ─── basic structs ──────────────────────────────────────────────────────────
struct Camera {
    pos: Vec3,
    angle: f32,
    fov: f32,
}
struct Texture {
    w: usize,
    h: usize,
    pix: Vec<u32>,
}

// ─── palette & patch decode ─────────────────────────────────────────────────
fn load_palette(wad: &Wad) -> Result<[u32; 256], Box<dyn Error>> {
    let idx = wad.find_lump("PLAYPAL").ok_or("PLAYPAL missing")?;
    let lump = wad.lump_bytes(idx);
    let mut pal = [0u32; 256];
    for i in 0..256 {
        pal[i] =
            (lump[i * 3] as u32) << 16 | (lump[i * 3 + 1] as u32) << 8 | lump[i * 3 + 2] as u32;
    }
    Ok(pal)
}
fn decode_patch(raw: &[u8], pal: &[u32; 256]) -> Texture {
    let w = u16::from_le_bytes([raw[0], raw[1]]) as usize;
    let h = u16::from_le_bytes([raw[2], raw[3]]) as usize;
    let mut pix = vec![0u32; w * h];
    let colofs = &raw[8..8 + w * 4];
    for x in 0..w {
        let mut p = u32::from_le_bytes(colofs[x * 4..][..4].try_into().unwrap()) as usize;
        loop {
            let row = raw[p] as usize;
            if row == 0xFF {
                break;
            }
            let len = raw[p + 1] as usize;
            p += 3;
            for i in 0..len {
                let y = row + i;
                pix[y * w + x] = pal[raw[p + i] as usize];
            }
            p += len + 1;
        }
    }
    Texture { w, h, pix }
}
fn checker() -> Texture {
    let (w, h) = (64, 64);
    let mut p = vec![0u32; w * h];
    for y in 0..h {
        for x in 0..w {
            p[y * w + x] = if (x / 8 + y / 8) & 1 == 0 {
                0x007F7FFF
            } else {
                0x00FF7F7F
            };
        }
    }
    Texture { w, h, pix: p }
}

// ─── build texture table (TEXTURE1/2 + PNAMES) ─────────────────────────────
fn build_texture_table(wad: &Wad, pal: &[u32; 256]) -> HashMap<String, Texture> {
    // PNAMES
    let pbytes = wad.lump_bytes(wad.find_lump("PNAMES").unwrap());
    let n_patches = u32::from_le_bytes(pbytes[0..4].try_into().unwrap()) as usize;
    let mut patch_names = Vec::with_capacity(n_patches);
    for i in 0..n_patches {
        patch_names.push(norm(lump_name(&pbytes[4 + i * 8..])));
    }

    // load patches
    let mut patches = HashMap::<String, Texture>::new();
    for n in &patch_names {
        if let Some(idx) = wad.find_lump(n) {
            patches.insert(n.clone(), decode_patch(wad.lump_bytes(idx), pal));
        }
    }

    // compose textures
    let mut tbl = HashMap::<String, Texture>::new();
    for tl in ["TEXTURE1", "TEXTURE2"] {
        if let Some(idx) = wad.find_lump(tl) {
            let lump = wad.lump_bytes(idx);
            let mut cur = Cursor::new(lump);
            let ntex = read_i32(&mut cur) as usize;
            let mut offs = Vec::with_capacity(ntex);
            for _ in 0..ntex {
                offs.push(read_i32(&mut cur) as usize);
            }
            for off in offs {
                let mut c = Cursor::new(&lump[off..]);
                let name = norm(lump_name(read_name(&mut c)));
                let _masked = read_i32(&mut c);
                let w = read_i16(&mut c) as usize;
                let h = read_i16(&mut c) as usize;
                let _col_dir = read_i32(&mut c);
                let np = read_i16(&mut c) as usize;

                let mut canv = vec![0u32; w * h];
                for _ in 0..np {
                    let ox = read_i16(&mut c) as i32;
                    let oy = read_i16(&mut c) as i32;
                    let pnum = read_i16(&mut c) as usize;
                    let _step = read_i16(&mut c);
                    let _colmap = read_i16(&mut c);
                    if let Some(p) = patches.get(patch_names[pnum].as_str()) {
                        blit_patch(&mut canv, w, h, p, ox, oy);
                    }
                }
                tbl.insert(name, Texture { w, h, pix: canv });
            }
        }
    }
    tbl.insert("CHECKER".into(), checker());
    tbl
}
fn blit_patch(canv: &mut [u32], w: usize, h: usize, p: &Texture, ox: i32, oy: i32) {
    for py in 0..p.h {
        let cy = oy + py as i32;
        if cy < 0 || cy >= h as i32 {
            continue;
        }
        for px in 0..p.w {
            let cx = ox + px as i32;
            if cx < 0 || cx >= w as i32 {
                continue;
            }
            let col = p.pix[py * p.w + px];
            if col != 0 {
                canv[cy as usize * w + cx as usize] = col;
            }
        }
    }
}

// ─── BSP traversal + render ─────────────────────────────────────────────────
enum Child {
    Node(u16),
    Sub(u16),
}
impl From<u16> for Child {
    fn from(x: u16) -> Self {
        if x & 0x8000 != 0 {
            Self::Sub(x & 0x7FFF)
        } else {
            Self::Node(x)
        }
    }
}
fn point_side(p: Vec2, n: &Node) -> i32 {
    let d = (p.x - n.x as f32) * n.dy as f32 - (p.y - n.y as f32) * n.dx as f32;
    if d >= 0.0 { 0 } else { 1 }
}

// ─── main ───────────────────────────────────────────────────────────────────
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let wad_path = args.next().expect("usage: textures_3d <wad> [map]");
    let map_idx: usize = args.next().unwrap_or_else(|| "0".into()).parse()?;

    let wad = Wad::from_file(&wad_path)?;
    let level = wad.parse_level(wad.level_indices()[map_idx])?;
    let palette = load_palette(&wad)?;
    let tex_tbl = build_texture_table(&wad, &palette);

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

    let mut frame = vec![0u32; WIDTH * HEIGHT];
    let mut solidx = vec![false; WIDTH];
    let mut win = Window::new(
        "Textures – fixed vertical",
        WIDTH,
        HEIGHT,
        WindowOptions::default(),
    )?;
    win.set_target_fps(35);

    let speed = 100.0;
    let rot = std::f32::consts::PI;
    let dt = 1.0 / 35.0;
    let root = level.nodes.len() - 1;

    while win.is_open() && !win.is_key_down(Key::Escape) {
        handle_input(&mut cam, &win, speed, rot, dt);

        frame.fill(0x00202020);
        solidx.fill(false);

        render_rec(
            Child::Node(root as u16),
            &cam,
            &level,
            &tex_tbl,
            &mut frame,
            &mut solidx,
        );

        win.update_with_buffer(&frame, WIDTH, HEIGHT)?;
    }
    Ok(())
}

// ─── recursive BSP render ───────────────────────────────────────────────────
fn render_rec(
    child: Child,
    cam: &Camera,
    lvl: &Level,
    tex_tbl: &HashMap<String, Texture>,
    buf: &mut [u32],
    occ: &mut [bool],
) {
    match child {
        Child::Sub(ss) => {
            if let Some(ssec) = lvl.subsectors.get(ss as usize) {
                for i in 0..ssec.seg_count {
                    draw_seg(cam, lvl, (ssec.first_seg + i) as usize, tex_tbl, buf, occ);
                }
            }
        }
        Child::Node(n) => {
            let node = &lvl.nodes[n as usize];
            let side = point_side(cam.pos.xy(), node) as usize;
            render_rec(Child::from(node.child[side]), cam, lvl, tex_tbl, buf, occ);
            render_rec(
                Child::from(node.child[side ^ 1]),
                cam,
                lvl,
                tex_tbl,
                buf,
                occ,
            );
        }
    }
}

// ─── draw single SEG with correct perspective ──────────────────────────────
fn draw_seg(
    cam: &Camera,
    lvl: &Level,
    id: usize,
    tex_tbl: &HashMap<String, Texture>,
    buf: &mut [u32],
    occ: &mut [bool],
) {
    let seg = &lvl.segs[id];
    let v1 = &lvl.vertices[seg.v1 as usize];
    let v2 = &lvl.vertices[seg.v2 as usize];
    let w1 = vec2(v1.x as f32, v1.y as f32) - cam.pos.xy();
    let w2 = vec2(v2.x as f32, v2.y as f32) - cam.pos.xy();
    let (sin, cos) = cam.angle.sin_cos();
    let p1 = Vec2::new(w1.x * cos + w1.y * sin, -w1.x * sin + w1.y * cos);
    let p2 = Vec2::new(w2.x * cos + w2.y * sin, -w2.x * sin + w2.y * cos);

    if p1.y <= NEAR && p2.y <= NEAR {
        return;
    }
    let (p1, p2, t1, t2) = clip_near_tex(p1, p2);

    // screen params
    let hw = WIDTH as f32 * 0.5;
    let hh = HEIGHT as f32 * 0.5;
    let focal = hw / (cam.fov * 0.5).tan();
    let sx1 = hw + p1.x * focal / p1.y;
    let sx2 = hw + p2.x * focal / p2.y;
    let (mut a, mut b) = (sx1 as i32, sx2 as i32);
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    let (ix1, ix2) = (a.clamp(0, WIDTH as i32 - 1), b.clamp(0, WIDTH as i32 - 1));
    if ix1 == ix2 {
        return;
    }

    // sidedef & texture
    let ld = &lvl.linedefs[seg.linedef as usize];
    let (sd_idx, solid) = if seg.dir == 0 {
        (ld.right, ld.left == NO_SIDE)
    } else {
        (ld.left, ld.right == NO_SIDE)
    };
    if sd_idx == NO_SIDE {
        return;
    }
    let sd = &lvl.sidedefs[sd_idx as usize];
    let raw_name = str_from(&sd.middle);
    if raw_name == "-" || raw_name.trim().is_empty() {
        return;
    }
    let tex = tex_tbl
        .get(&norm(raw_name))
        .unwrap_or_else(|| tex_tbl.get("CHECKER").unwrap());

    // sector heights
    let sec = &lvl.sectors[sd.sector as usize];
    let eye = cam.pos.z;

    // pre-compute u/z and 1/z at endpoints
    let wall_len = (w2 - w1).length();
    let u1 = sd.x_off as f32 + t1 * wall_len;
    let u2 = sd.x_off as f32 + t2 * wall_len;
    let invz1 = 1.0 / p1.y;
    let invz2 = 1.0 / p2.y;
    let uo_z1 = u1 * invz1;
    let uo_z2 = u2 * invz2;

    for x in ix1..=ix2 {
        if occ[x as usize] {
            continue;
        }
        let t = (x as f32 - sx1) / (sx2 - sx1);
        let invz = invz1 + (invz2 - invz1) * t;
        let u = ((uo_z1 + (uo_z2 - uo_z1) * t) / invz).rem_euclid(tex.w as f32) as usize;

        let top = hh - (sec.ceil as f32 - eye) * focal * invz;
        let bot = hh - (sec.floor as f32 - eye) * focal * invz;
        let (ya, yb) = (top as i32, bot as i32);
        if ya >= HEIGHT as i32 || yb < 0 || ya >= yb {
            continue;
        }
        let y0 = ya.clamp(0, HEIGHT as i32 - 1);
        let y1 = yb.clamp(0, HEIGHT as i32 - 1);

        for y in y0..=y1 {
            let row_frac = (y - ya) as f32 / (yb - ya) as f32; // 0.0 .. 1.0
            let mut v = (row_frac * tex.h as f32) as i32 + sd.y_off as i32;
            v = v.rem_euclid(tex.h as i32); // wrap
            let v = v as usize;
            buf[y as usize * WIDTH + x as usize] = tex.pix[v * tex.w + u];
        }
        if solid {
            occ[x as usize] = true;
        }
    }
}

// clip wall against near plane; return endpoints + param t along original seg
fn clip_near_tex(mut p1: Vec2, mut p2: Vec2) -> (Vec2, Vec2, f32, f32) {
    let (mut t1, mut t2) = (0.0, 1.0);
    if p1.y < NEAR {
        let t = (NEAR - p1.y) / (p2.y - p1.y);
        p1 = Vec2::new(p1.x + (p2.x - p1.x) * t, NEAR);
        t1 = t;
    }
    if p2.y < NEAR {
        let t = (NEAR - p2.y) / (p1.y - p2.y);
        p2 = Vec2::new(p2.x + (p1.x - p2.x) * t, NEAR);
        t2 = 1.0 - t;
    }
    (p1, p2, t1, t2)
}

// ─── input ──────────────────────────────────────────────────────────────────
fn handle_input(cam: &mut Camera, win: &Window, speed: f32, rot: f32, dt: f32) {
    if win.is_key_down(Key::Left) {
        cam.angle += rot * dt;
    }
    if win.is_key_down(Key::Right) {
        cam.angle -= rot * dt;
    }
    let (sin, cos) = cam.angle.sin_cos();
    let (mut dx, mut dy) = (0.0, 0.0);
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
