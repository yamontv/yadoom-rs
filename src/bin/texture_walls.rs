//! Textured Doom-map renderer
//! – perspective-correct walls
//! – camera follows floor height
//! – simple solid-wall collision (slide)
//!
//! Controls  W/S forward · A/D strafe · ←/→ turn · Esc quit
//!
//! ```bash
//! cargo run --release --bin textures_3d -- assets/doom.wad 1
//! ```

use glam::{Vec2, Vec3, Vec3Swizzles, vec2};
use minifb::{Key, Window, WindowOptions};
use std::{
    collections::HashMap,
    error::Error,
    io::{Cursor, Read},
};
use wad::{Wad, level::*};

// ─── globals ────────────────────────────────────────────────────────────────
const WIDTH: usize = 640;
const HEIGHT: usize = 400;

const HFOV: f32 = 110.0_f32.to_radians(); // 110 °
const NEAR: f32 = 1.0; // near plane in map units

const NO_SIDE: u16 = 0xFFFF;

const EYE_HEIGHT: f32 = 41.0; // Doomguy’s eye above floor
const PLAYER_R: f32 = 16.0; // collision radius

// placeholder flat colours
const CEIL_COL: u32 = 0x00404040;
const FLOOR_COL: u32 = 0x00602020;

// ─── tiny helpers ───────────────────────────────────────────────────────────
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
fn r16(c: &mut Cursor<&[u8]>) -> i16 {
    let mut b = [0; 2];
    c.read_exact(&mut b).unwrap();
    i16::from_le_bytes(b)
}
fn r32(c: &mut Cursor<&[u8]>) -> i32 {
    let mut b = [0; 4];
    c.read_exact(&mut b).unwrap();
    i32::from_le_bytes(b)
}
fn rname<'a>(c: &mut Cursor<&'a [u8]>) -> &'a [u8] {
    let p = c.position() as usize;
    c.set_position((p + 8) as u64);
    &c.get_ref()[p..p + 8]
}

// ─── basic structs ──────────────────────────────────────────────────────────
struct Camera {
    pos: Vec3,
    ang: f32,
}
struct Texture {
    w: usize,
    h: usize,
    pix: Vec<u32>,
}

// ─── palette & patch decode ─────────────────────────────────────────────────
fn load_palette(w: &Wad) -> [u32; 256] {
    let lump = w.lump_bytes(w.find_lump("PLAYPAL").expect("PLAYPAL"));
    let mut pal = [0u32; 256];
    for i in 0..256 {
        pal[i] =
            (lump[i * 3] as u32) << 16 | (lump[i * 3 + 1] as u32) << 8 | lump[i * 3 + 2] as u32;
    }
    pal
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
fn build_textures(w: &Wad, pal: &[u32; 256]) -> HashMap<String, Texture> {
    // ---- PNAMES
    let pbytes = w.lump_bytes(w.find_lump("PNAMES").unwrap());
    let n = u32::from_le_bytes(pbytes[0..4].try_into().unwrap()) as usize;
    let mut pnames = Vec::with_capacity(n);
    for i in 0..n {
        pnames.push(norm(lump_name(&pbytes[4 + i * 8..])));
    }
    let mut patches = HashMap::<String, Texture>::new();
    for n in &pnames {
        if let Some(idx) = w.find_lump(n) {
            patches.insert(n.clone(), decode_patch(w.lump_bytes(idx), pal));
        }
    }

    // ---- TEXTURE lumps
    let mut tbl = HashMap::<String, Texture>::new();
    for tl in ["TEXTURE1", "TEXTURE2"] {
        if let Some(idx) = w.find_lump(tl) {
            let lump = w.lump_bytes(idx);
            let mut c = Cursor::new(lump);
            let ntex = r32(&mut c) as usize;
            let mut offs = Vec::with_capacity(ntex);
            for _ in 0..ntex {
                offs.push(r32(&mut c) as usize);
            }
            for off in offs {
                let mut cur = Cursor::new(&lump[off..]);
                let name = norm(lump_name(rname(&mut cur)));
                let _mask = r32(&mut cur);
                let w_tex = r16(&mut cur) as usize;
                let h_tex = r16(&mut cur) as usize;
                let _column = r32(&mut cur);
                let np = r16(&mut cur) as usize;
                let mut canv = vec![0u32; w_tex * h_tex];
                for _ in 0..np {
                    let ox = r16(&mut cur) as i32;
                    let oy = r16(&mut cur) as i32;
                    let pidx = r16(&mut cur) as usize;
                    let _step = r16(&mut cur);
                    let _cm = r16(&mut cur);
                    if let Some(p) = patches.get(&pnames[pidx]) {
                        // blit
                        for py in 0..p.h {
                            let cy = oy + py as i32;
                            if cy < 0 || cy >= h_tex as i32 {
                                continue;
                            }
                            for px in 0..p.w {
                                let cx = ox + px as i32;
                                if cx < 0 || cx >= w_tex as i32 {
                                    continue;
                                }
                                let col = p.pix[py * p.w + px];
                                if col != 0 {
                                    canv[cy as usize * w_tex + cx as usize] = col;
                                }
                            }
                        }
                    }
                }
                tbl.insert(
                    name,
                    Texture {
                        w: w_tex,
                        h: h_tex,
                        pix: canv,
                    },
                );
            }
        }
    }
    tbl.insert("CHECKER".into(), checker());
    tbl
}

// ─── build flat table (F_START .. F_END) ───────────────────────────────────
fn build_flats(w: &Wad, pal: &[u32; 256]) -> HashMap<String, Texture> {
    let mut tbl = HashMap::<String, Texture>::new();
    let mut in_flats = false;
    for (i, li) in w.lumps.iter().enumerate() {
        let name = Wad::lump_name(&li.name);
        match name {
            "F_START" | "FF_START" => {
                in_flats = true;
                continue;
            }
            "F_END" | "FF_END" => {
                in_flats = false;
                continue;
            }
            _ => {}
        }
        if !in_flats {
            continue;
        }
        let lump = w.lump_bytes(i);
        if lump.len() == 4096 {
            // 64×64 raw flat
            let mut pix = vec![0u32; 64 * 64];
            for (j, p) in lump.iter().enumerate() {
                pix[j] = pal[*p as usize];
            }
            tbl.insert(name.to_string(), Texture { w: 64, h: 64, pix });
        }
    }
    tbl.insert("CHECKER".into(), checker());
    tbl
}

// ─── BSP helpers (locate subsector) ─────────────────────────────────────────
fn locate_subsector(p: Vec2, lvl: &Level) -> usize {
    let mut node = lvl.nodes.len() - 1;
    loop {
        let n = &lvl.nodes[node];
        let s = ((p.x - n.x as f32) * n.dy as f32 - (p.y - n.y as f32) * n.dx as f32) < 0.0;
        let child = n.child[s as usize];
        if child & 0x8000 != 0 {
            return (child & 0x7FFF) as usize;
        }
        node = child as usize;
    }
}
fn floor_height(ss: &Subsector, lvl: &Level) -> f32 {
    let seg = &lvl.segs[ss.first_seg as usize];
    let ld = &lvl.linedefs[seg.linedef as usize];
    let sd_idx = if seg.dir == 0 { ld.right } else { ld.left };
    let sector = &lvl.sectors[lvl.sidedefs[sd_idx as usize].sector as usize];
    sector.floor as f32
}

// ─── collision vs. one-sided linedefs ───────────────────────────────────────
fn blocked(pt: Vec2, lvl: &Level) -> bool {
    for ld in &lvl.linedefs {
        if ld.left == NO_SIDE || ld.right == NO_SIDE {
            let a = vec2(
                lvl.vertices[ld.v1 as usize].x as f32,
                lvl.vertices[ld.v1 as usize].y as f32,
            );
            let b = vec2(
                lvl.vertices[ld.v2 as usize].x as f32,
                lvl.vertices[ld.v2 as usize].y as f32,
            );
            let ab = b - a;
            let proj = ((pt - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
            let closest = a + ab * proj;
            if (pt - closest).length() < PLAYER_R {
                return true;
            }
        }
    }
    false
}

// ─── BSP traversal ──────────────────────────────────────────────────────────
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

// ─── rendering – perspective-correct walls ─────────────────────────────────
fn draw_seg(
    cam: &Camera,
    lvl: &Level,
    id: usize,
    tex: &HashMap<String, Texture>,
    buf: &mut [u32],
    occ: &mut [bool],
    ceil_clip: &mut [i32],
    floor_clip: &mut [i32],
) {
    let seg = &lvl.segs[id];
    let v1 = &lvl.vertices[seg.v1 as usize];
    let v2 = &lvl.vertices[seg.v2 as usize];
    let w1 = vec2(v1.x as f32, v1.y as f32) - cam.pos.xy();
    let w2 = vec2(v2.x as f32, v2.y as f32) - cam.pos.xy();
    let (sin, cos) = cam.ang.sin_cos();
    let p1 = Vec2::new(w1.x * cos + w1.y * sin, -w1.x * sin + w1.y * cos);
    let p2 = Vec2::new(w2.x * cos + w2.y * sin, -w2.x * sin + w2.y * cos);
    if p1.y <= NEAR && p2.y <= NEAR {
        return;
    }

    // near-plane clip
    let (p1, p2, t1, t2) = {
        let (mut q1, mut q2) = (p1, p2);
        let (mut u1, mut u2) = (0.0, 1.0);
        if q1.y < NEAR {
            let t = (NEAR - q1.y) / (q2.y - q1.y);
            q1 = Vec2::new(q1.x + (q2.x - q1.x) * t, NEAR);
            u1 = t;
        }
        if q2.y < NEAR {
            let t = (NEAR - q2.y) / (q1.y - q2.y);
            q2 = Vec2::new(q2.x + (q1.x - q2.x) * t, NEAR);
            u2 = 1.0 - t;
        }
        (q1, q2, u1, u2)
    };

    // screen X span
    let hw = WIDTH as f32 * 0.5;
    let hh = HEIGHT as f32 * 0.5;
    let focal = hw / (HFOV * 0.5).tan();
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
    let raw = str_from(&sd.middle);
    if raw == "-" || raw.trim().is_empty() {
        return;
    }
    let tex = tex
        .get(&norm(raw))
        .unwrap_or_else(|| tex.get("CHECKER").unwrap());
    let sec = &lvl.sectors[sd.sector as usize];

    // perspective coeffs
    let wall_len = (w2 - w1).length();
    let u1 = sd.x_off as f32 + t1 * wall_len;
    let u2 = sd.x_off as f32 + t2 * wall_len;
    let invz1 = 1.0 / p1.y;
    let invz2 = 1.0 / p2.y;
    let uoz1 = u1 * invz1;
    let uoz2 = u2 * invz2;

    let eye = cam.pos.z;

    for x in ix1..=ix2 {
        if occ[x as usize] {
            continue;
        }
        let t = (x as f32 - sx1) / (sx2 - sx1);
        let invz = invz1 + (invz2 - invz1) * t;
        let u = (uoz1 + (uoz2 - uoz1) * t) / invz;
        let u = u.rem_euclid(tex.w as f32) as usize;

        let top = hh - (sec.ceil as f32 - eye) * focal * invz;
        let bot = hh - (sec.floor as f32 - eye) * focal * invz;
        let (ya, yb) = (top as i32, bot as i32);
        if ya >= HEIGHT as i32 || yb < 0 || ya >= yb {
            continue;
        }
        let y0 = ya.clamp(0, HEIGHT as i32 - 1);
        let y1 = yb.clamp(0, HEIGHT as i32 - 1);
        let wall_h = (yb - ya) as f32;

        for y in y0..=y1 {
            let frac = (y - ya) as f32 / wall_h;
            let v =
                ((frac * tex.h as f32) as i32 + sd.y_off as i32).rem_euclid(tex.h as i32) as usize;
            let col = tex.pix[v * tex.w + u];
            if col != 0 {
                buf[y as usize * WIDTH + x as usize] = col;
            }
        }

        // ---- new: flat fill above & below this column ----------------
        // ceiling band
        if ya > ceil_clip[x as usize] {
            for y in ceil_clip[x as usize]..ya {
                buf[y as usize * WIDTH + x as usize] = CEIL_COL;
            }
            ceil_clip[x as usize] = ya;
        }
        // floor band
        if yb < floor_clip[x as usize] {
            for y in (yb + 1)..=floor_clip[x as usize] {
                buf[y as usize * WIDTH + x as usize] = FLOOR_COL;
            }
            floor_clip[x as usize] = yb;
        }

        if solid {
            occ[x as usize] = true;
        }
    }
}

// ─── render BSP recursively ────────────────────────────────────────────────
fn render(
    child: Child,
    cam: &Camera,
    lvl: &Level,
    tex: &HashMap<String, Texture>,
    buf: &mut [u32],
    occ: &mut [bool],
    ceil_clip: &mut [i32],
    floor_clip: &mut [i32],
) {
    match child {
        Child::Sub(ss) => {
            if let Some(s) = lvl.subsectors.get(ss as usize) {
                for i in 0..s.seg_count {
                    draw_seg(
                        cam,
                        lvl,
                        (s.first_seg + i) as usize,
                        tex,
                        buf,
                        occ,
                        ceil_clip,
                        floor_clip,
                    );
                }
            }
        }
        Child::Node(n) => {
            let node = &lvl.nodes[n as usize];
            let side = point_side(cam.pos.xy(), node) as usize;
            render(
                Child::from(node.child[side]),
                cam,
                lvl,
                tex,
                buf,
                occ,
                ceil_clip,
                floor_clip,
            );
            render(
                Child::from(node.child[side ^ 1]),
                cam,
                lvl,
                tex,
                buf,
                occ,
                ceil_clip,
                floor_clip,
            );
        }
    }
}

// ─── main ───────────────────────────────────────────────────────────────────
fn main() -> Result<(), Box<dyn Error>> {
    // CLI
    let mut args = std::env::args().skip(1);
    let wad_path = args.next().expect("usage: textures_3d <wad> [map]");
    let map_idx: usize = args.next().unwrap_or_else(|| "0".into()).parse()?;

    // load
    let wad = Wad::from_file(&wad_path)?;
    let level = wad.parse_level(wad.level_indices()[map_idx])?;
    let palette = load_palette(&wad);
    let textures = build_textures(&wad, &palette);
    let flats = build_flats(&wad, &palette);

    // spawn
    let (px, py, ang) = level
        .things
        .iter()
        .find(|t| t.type_ == 1)
        .map(|t| (t.x as f32, t.y as f32, (t.angle as f32).to_radians()))
        .unwrap_or((0.0, 0.0, 0.0));
    let ss0 = locate_subsector(vec2(px, py), &level);
    let floor_start = floor_height(&level.subsectors[ss0], &level);
    let mut cam = Camera {
        pos: Vec3::new(px, py, floor_start + EYE_HEIGHT),
        ang,
    };

    // buffers & window
    let mut frame = vec![0u32; WIDTH * HEIGHT];
    let mut occ = vec![false; WIDTH];
    let mut win = Window::new(
        "Textured Doom – floor follow",
        WIDTH,
        HEIGHT,
        WindowOptions::default(),
    )?;
    win.set_target_fps(35);

    // consts
    let speed = 100.0;
    let rot = std::f32::consts::PI;
    let dt = 1.0 / 35.0;
    let root = level.nodes.len() - 1;

    let mut ceil_clip = vec![0i32; WIDTH];
    let mut floor_clip = vec![HEIGHT as i32 - 1; WIDTH];

    let hw = WIDTH as f32 * 0.5;
    let hh = HEIGHT as f32 * 0.5;
    let focal = hw / (HFOV * 0.5).tan();

    // main loop
    while win.is_open() && !win.is_key_down(Key::Escape) {
        // ─ movement intent
        let (sin, cos) = cam.ang.sin_cos();
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

        // rotate
        if win.is_key_down(Key::Left) {
            cam.ang += rot * dt;
        }
        if win.is_key_down(Key::Right) {
            cam.ang -= rot * dt;
        }

        // desired step
        // let step = vec2(cos * dy - sin * dx, sin * dy + cos * dx);
        let step = vec2(-sin * dy - cos * dx, cos * dy - sin * dx);

        // collision & slide
        let try_pt = cam.pos.xy() + step;
        let pass_full = !blocked(try_pt, &level);
        let pass_x = !blocked(vec2(try_pt.x, cam.pos.y), &level);
        let pass_y = !blocked(vec2(cam.pos.x, try_pt.y), &level);
        if pass_full {
            cam.pos.x = try_pt.x;
            cam.pos.y = try_pt.y;
        } else {
            if pass_x {
                cam.pos.x = try_pt.x;
            }
            if pass_y {
                cam.pos.y = try_pt.y;
            }
        }

        // floor follow
        let ss = locate_subsector(cam.pos.xy(), &level);
        cam.pos.z = floor_height(&level.subsectors[ss], &level) + EYE_HEIGHT;

        let sector = &level.sectors[level.sidedefs[level.linedefs
            [level.segs[level.subsectors[ss].first_seg as usize].linedef as usize]
            .right as usize]
            .sector as usize];
        let floor_flat = flats
            .get(&norm(str_from(&sector.floor_tex)))
            .unwrap_or(&flats["CHECKER"]);
        let ceil_flat = flats
            .get(&norm(str_from(&sector.ceil_tex)))
            .unwrap_or(&flats["CHECKER"]);

        ceil_clip.fill(0); // NEW
        floor_clip.fill(HEIGHT as i32 - 1); // NEW

        // ─ render
        frame.fill(0x00202020);
        occ.fill(false);
        render(
            Child::Node(root as u16),
            &cam,
            &level,
            &textures,
            &mut frame,
            &mut occ,
            &mut ceil_clip,
            &mut floor_clip,
        );

        for x in 0..WIDTH {
            let dir_x_cam = (x as f32 - hw) / focal;
            for y in 0..ceil_clip[x] as usize {
                // —— ceiling pixel ——
                let depth = (sector.ceil as f32 - cam.pos.z) * focal / (hh - y as f32);
                let dx = dir_x_cam;
                let dy = 1.0;
                let dir_world_x = dx * cam.ang.cos() - dy * cam.ang.sin();
                let dir_world_y = dx * cam.ang.sin() + dy * cam.ang.cos();
                let hit_x = cam.pos.x + dir_world_x * depth;
                let hit_y = cam.pos.y + dir_world_y * depth;
                let u = hit_x as i32 & 63;
                let v = hit_y as i32 & 63;
                frame[y * WIDTH + x] = ceil_flat.pix[v as usize * 64 + u as usize];
            }
            for y in (floor_clip[x] + 1) as usize..HEIGHT {
                // —— floor pixel ——
                let depth = (cam.pos.z - sector.floor as f32) * focal / (y as f32 - hh);
                let dx = dir_x_cam;
                let dy = 1.0;
                let dir_world_x = dx * cam.ang.cos() - dy * cam.ang.sin();
                let dir_world_y = dx * cam.ang.sin() + dy * cam.ang.cos();
                let hit_x = cam.pos.x + dir_world_x * depth;
                let hit_y = cam.pos.y + dir_world_y * depth;
                let u = hit_x as i32 & 63;
                let v = hit_y as i32 & 63;
                frame[y * WIDTH + x] = floor_flat.pix[v as usize * 64 + u as usize];
            }
        }

        win.update_with_buffer(&frame, WIDTH, HEIGHT)?;
    }
    Ok(())
}
