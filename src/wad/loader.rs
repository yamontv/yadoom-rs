// ──────────────────────────────────────────────────────────────────────────
// wad/loader.rs
//
//  *   RawLevel   (wad::level)           ──╮
//  *   world::Palette / patches  (from Wad)       │   --->  world::geometry::Level
//  *   world::TextureBank (mut)                   │          + populated world::TextureBank
//                                          ╯
// ──────────────────────────────────────────────────────────────────────────

use glam::{Vec2, vec2};
use thiserror::Error;

use super::level as raw_level;
use super::raw::{Wad, WadError};

use crate::world;

/*──────────────────────────── Error type ───────────────────────────*/

#[derive(Error, Debug)]
pub enum LoadError {
    #[error(transparent)]
    Wad(#[from] WadError),

    #[error(transparent)]
    Level(#[from] raw_level::LevelError),

    #[error(transparent)]
    Texture(#[from] world::TextureError),

    #[error("PLAYPAL lump missing - cannot build palette")]
    NoPalette,

    #[error("COLORMAP lump missing - cannot build palette")]
    NoColormap,

    #[error("S_START/END lump missing - cannot build sprites")]
    NoSprites,
}

/*====================================================================*/
/*                       Public API                                   */
/*====================================================================*/

/// Load the map at `marker` into a `world::Level` and populate `bank` with
/// every texture that map references.  Unknown names are replaced by the
/// bank’s checkerboard id (0).
pub fn load_level(
    wad: &Wad,
    marker: usize,
    bank: &mut world::TextureBank,
) -> Result<world::Level, LoadError> {
    /*----- 1. Raw lumps --------------------------------------------------*/
    let raw = wad.parse_level(marker)?;

    /*----- 2. world::Palette needed for patches + flats -------------------------*/
    let palette = load_palette(wad).ok_or(LoadError::NoPalette)?;

    bank.set_palette(palette);

    let colormap = load_colormap(wad).ok_or(LoadError::NoPalette)?;

    bank.set_colormap(colormap);

    bank.build_shade_table();

    load_all_sprites(wad, bank)?;

    /*----- 3. Patch cache (index → world::Texture) ------------------------------*/
    let patch_vec = decode_all_patches(wad)?;

    /*----- 4. Helper: resolve name → TextureId ---------------------------*/
    let mut tex_id = |name_bytes: &[u8; 8]| -> Result<TextureId, LoadError> {
        let name = Wad::lump_name_str(name_bytes).to_ascii_uppercase();
        if let Some(id) = bank.id(&name) {
            return Ok(id);
        }
        if let Some(tex) = build_wall_texture(wad, &patch_vec, &name) {
            return Ok(bank.insert(name, tex)?);
        }
        if let Some(tex) = decode_flat(wad, &name) {
            return Ok(bank.insert(name, tex)?);
        }
        Ok(world::NO_TEXTURE)
    };

    /*----- 5. Convert raw → geo lists ------------------------------------*/
    use world::*;

    let things: Vec<Thing> = raw.things.into_iter().map(raw_to_geo::thing_from).collect();

    let linedefs: Vec<world::Linedef> = raw
        .linedefs
        .into_iter()
        .enumerate()
        .map(|(idx, raw_ld)| {
            let v1 = &raw.vertices[raw_ld.v1 as usize];
            let v2 = &raw.vertices[raw_ld.v2 as usize];
            let bbox = world::Aabb {
                min: Vec2::new(v1.x.min(v2.x) as f32, v1.y.min(v2.y) as f32),
                max: Vec2::new(v1.x.max(v2.x) as f32, v1.y.max(v2.y) as f32),
            };
            raw_to_geo::linedef_from(raw_ld, idx as world::LinedefId, bbox)
        })
        .collect();

    let vertices: Vec<Vertex> = raw
        .vertices
        .into_iter()
        .map(raw_to_geo::vertex_from)
        .collect();

    let segs: Vec<Segment> = raw.segs.into_iter().map(raw_to_geo::seg_from).collect();

    let subsectors: Vec<Subsector> = raw
        .subsectors
        .into_iter()
        .map(raw_to_geo::subsector_from)
        .collect();

    let nodes: Vec<Node> = raw.nodes.into_iter().map(raw_to_geo::node_from).collect();

    /*----- lists that need texture look-ups (may fail) -------------------*/
    let sidedefs: Vec<Sidedef> = raw
        .sidedefs
        .into_iter()
        .map(|s| {
            Ok(Sidedef {
                x_off: s.x_off as f32,
                y_off: s.y_off as f32,
                upper: tex_id(&s.top_tex)?,
                lower: tex_id(&s.bottom_tex)?,
                middle: tex_id(&s.mid_tex)?,
                sector: s.sector as world::SectorId,
            })
        })
        .collect::<Result<_, LoadError>>()?;

    let sectors: Vec<Sector> = raw
        .sectors
        .into_iter()
        .map(|s| {
            Ok(Sector {
                floor_h: s.floor_h as f32,
                ceil_h: s.ceil_h as f32,
                floor_tex: tex_id(&s.floor_tex)?,
                ceil_tex: tex_id(&s.ceil_tex)?,
                light: f32::from(s.light >> 3) / 31.0,
                special: s.special,
                tag: s.tag,
            })
        })
        .collect::<Result<_, LoadError>>()?;

    let blockmap = raw_to_geo::blockmap_from(raw.blockmap);

    /*----- 6. Assemble world::Level -------------------------------------*/
    Ok(Level {
        name: raw.name,
        things,
        linedefs,
        sidedefs,
        vertices,
        segs,
        subsectors,
        nodes,
        sectors,
        blockmap,
    })
}

/*====================================================================*/
/*                  Raw → Geo helpers (local)                         */
/*====================================================================*/
mod raw_to_geo {
    use super::*;
    pub fn thing_from(r: raw_level::RawThing) -> world::Thing {
        let min_skill = match r.options & 0x0007 {
            0x0001 => 1,
            0x0002 => 2,
            0x0004 => 3,
            _ => 1,
        };
        world::Thing {
            pos: vec2(r.x as f32, r.y as f32),
            angle: (r.angle as f32).to_radians(),
            type_id: r.type_ as u16,
            min_skill,
            is_deaf: r.options & 0x0020 != 0,
            multiplayer: r.options & 0x0100 != 0,
            sub_sector: u16::MAX,
        }
    }

    pub fn linedef_from(
        r: raw_level::RawLinedef,
        id: world::LinedefId,
        bbox: world::Aabb,
    ) -> world::Linedef {
        world::Linedef {
            id,
            v1: r.v1 as world::VertexId,
            v2: r.v2 as world::VertexId,
            flags: world::LinedefFlags::from_bits_truncate(r.flags as u16),
            special: r.special as u16,
            tag: r.tag as u16,
            right_sidedef: (r.sidenum[0] >= 0).then_some(r.sidenum[0] as world::SidedefId),
            left_sidedef: (r.sidenum[1] >= 0).then_some(r.sidenum[1] as world::SidedefId),
            bbox,
        }
    }

    pub fn vertex_from(r: raw_level::RawVertex) -> world::Vertex {
        world::Vertex {
            pos: vec2(r.x as f32, r.y as f32),
        }
    }
    pub fn seg_from(r: raw_level::RawSeg) -> world::Segment {
        world::Segment {
            v1: r.v1 as world::VertexId,
            v2: r.v2 as world::VertexId,
            linedef: r.linedef as world::LinedefId,
            dir: r.side as u16,
            offset: r.offset as f32,
        }
    }
    pub fn subsector_from(r: raw_level::RawSubsector) -> world::Subsector {
        world::Subsector {
            num_lines: r.seg_count as u16,
            first_line: r.first_seg as world::SegmentId,
            sector: u16::MAX,
            things: Vec::new(),
        }
    }

    const BOXTOP: usize = 0;
    const BOXBOTTOM: usize = 1;
    const BOXLEFT: usize = 2;
    const BOXRIGHT: usize = 3;

    #[inline]
    fn raw_bbox_to_aabb(raw: &[i16; 4]) -> world::Aabb {
        world::Aabb {
            min: Vec2::new(raw[BOXLEFT] as f32, raw[BOXBOTTOM] as f32),
            max: Vec2::new(raw[BOXRIGHT] as f32, raw[BOXTOP] as f32),
        }
    }

    pub fn node_from(r: raw_level::RawNode) -> world::Node {
        world::Node {
            x: r.x as f32,
            y: r.y as f32,
            dx: r.dx as f32,
            dy: r.dy as f32,
            bbox: [raw_bbox_to_aabb(&r.bbox[0]), raw_bbox_to_aabb(&r.bbox[1])],
            child: r.child,
        }
    }

    pub fn blockmap_from(r: raw_level::RawBlockmap) -> world::Blockmap {
        let cell_cnt = (r.width as usize) * (r.height as usize);
        let data_base = 4 + cell_cnt as i16; // header + offset table

        let mut bm_lines: Vec<Vec<world::LinedefId>> = vec![Vec::new(); cell_cnt];

        for (cell, &off) in r.offsets.iter().enumerate() {
            // convert lump-relative word offset → index into `r.data`
            let mut i = (off - data_base) as usize; // ← FIX ❶
            while i < r.data.len() {
                let v = r.data[i];
                if v == -1 {
                    break;
                }
                bm_lines[cell].push(v as world::LinedefId);
                i += 1;
            }
        }

        world::Blockmap {
            origin: vec2(r.origin_x as f32, r.origin_y as f32),
            width: r.width as i32,
            height: r.height as i32,
            lines: bm_lines,
        }
    }
}

/*====================================================================*/
/*                  world::Palette / patch / texture helpers                 */
/*====================================================================*/
fn load_palette(wad: &Wad) -> Option<world::Palette> {
    let idx = wad.find_lump("PLAYPAL")?;
    let bytes = wad.lump_bytes(idx).ok()?;
    let mut pal = world::Palette::default();
    for i in 0..256 {
        pal[i] =
            (bytes[i * 3] as u32) << 16 | (bytes[i * 3 + 1] as u32) << 8 | bytes[i * 3 + 2] as u32;
    }
    Some(pal)
}

fn load_colormap(wad: &Wad) -> Option<world::Colormap> {
    // 1) Find the lump index
    let idx = wad.find_lump("COLORMAP")?;

    // 2) Read its raw bytes
    let bytes = wad.lump_bytes(idx).ok()?;

    // 3) There should be at least 34 * 256 = 8704 bytes
    if bytes.len() < 34 * 256 {
        return None;
    }

    // 4) Allocate the array-of-arrays
    let mut cm = world::Colormap::default();

    // 5) Copy each 256-byte slice into its table
    for table in 0..34 {
        let start = table * 256;
        let end = start + 256;
        cm[table].copy_from_slice(&bytes[start..end]);
    }

    Some(cm)
}

/*-------------------- patch cache -----------------------------------*/

fn decode_all_patches(wad: &Wad) -> Result<Vec<world::Texture>, WadError> {
    let idx = wad.find_lump("PNAMES").unwrap();
    let bytes = wad.lump_bytes(idx)?;
    let num = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;

    let mut vec = Vec::with_capacity(num);
    for i in 0..num {
        let name_bytes: &[u8; 8] = (&bytes[4 + i * 8..4 + i * 8 + 8]).try_into().unwrap();
        let name = Wad::lump_name_str(name_bytes);
        if let Some(id) = wad.find_lump(name) {
            vec.push(decode_patch(name, wad.lump_bytes(id)?));
        } else {
            vec.push(world::Texture::default()); // unlikely but keeps indices aligned
        }
    }
    Ok(vec)
}

fn decode_patch(name: &str, raw: &[u8]) -> world::Texture {
    let w = u16::from_le_bytes(raw[0..2].try_into().unwrap()) as usize;
    let h = u16::from_le_bytes(raw[2..4].try_into().unwrap()) as usize;
    let mut pix = vec![0u8; w * h];
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
                pix[(row + i) * w + x] = raw[p + i];
            }
            p += len + 1;
        }
    }
    world::Texture {
        name: name.into(),
        w,
        h,
        pixels: pix,
    }
}

/*-------------------- wall texture compose --------------------------*/

fn build_wall_texture(wad: &Wad, patches: &[world::Texture], name: &str) -> Option<world::Texture> {
    for table in ["TEXTURE1", "TEXTURE2"] {
        let Some(idx) = wad.find_lump(table) else {
            continue;
        };
        let bytes = wad.lump_bytes(idx).ok()?;
        let ntex = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        let mut offs = &bytes[4..];
        for _ in 0..ntex {
            let off = u32::from_le_bytes(offs[0..4].try_into().unwrap()) as usize;
            offs = &offs[4..];
            let entry = &bytes[off..];
            let e_name_bytes: &[u8; 8] = (&entry[0..8]).try_into().unwrap();
            let e_name = Wad::lump_name_str(e_name_bytes);
            if !e_name.eq_ignore_ascii_case(name) {
                continue;
            }
            return Some(compose_texture(e_name, entry, patches));
        }
    }
    None
}

fn compose_texture(name: &str, entry: &[u8], patches: &[world::Texture]) -> world::Texture {
    let w_tex = i16::from_le_bytes(entry[12..14].try_into().unwrap()) as usize;
    let h_tex = i16::from_le_bytes(entry[14..16].try_into().unwrap()) as usize;
    let np = u16::from_le_bytes(entry[20..22].try_into().unwrap()) as usize;

    let mut canvas = vec![0u8; w_tex * h_tex];
    let mut pinfo = &entry[22..];
    for _ in 0..np {
        let ox = i16::from_le_bytes(pinfo[0..2].try_into().unwrap()) as i32;
        let oy = i16::from_le_bytes(pinfo[2..4].try_into().unwrap()) as i32;
        let idx = u16::from_le_bytes(pinfo[4..6].try_into().unwrap()) as usize;
        blit_patch(&mut canvas, w_tex, h_tex, &patches[idx], ox, oy);
        pinfo = &pinfo[10..];
    }
    world::Texture {
        name: name.into(),
        w: w_tex,
        h: h_tex,
        pixels: canvas,
    }
}

fn blit_patch(dest: &mut [u8], dw: usize, dh: usize, p: &world::Texture, ox: i32, oy: i32) {
    for py in 0..p.h {
        let dy = oy + py as i32;
        if !(0..dh as i32).contains(&dy) {
            continue;
        }
        for px in 0..p.w {
            let dx = ox + px as i32;
            if !(0..dw as i32).contains(&dx) {
                continue;
            }
            let src = p.pixels[py * p.w + px];
            if src != 0 {
                dest[dy as usize * dw + dx as usize] = src;
            }
        }
    }
}

/*----------------------------- flats --------------------------------*/

fn decode_flat(wad: &Wad, name: &str) -> Option<world::Texture> {
    let idx = wad.find_lump(name)?;
    let bytes = wad.lump_bytes(idx).ok()?;
    if bytes.len() != 4096 {
        return None;
    }
    let mut rgba = Vec::with_capacity(64 * 64);
    for &b in bytes {
        rgba.push(b);
    }
    Some(world::Texture {
        name: name.into(),
        w: 64,
        h: 64,
        pixels: rgba,
    })
}

fn load_all_sprites(wad: &Wad, bank: &mut world::TextureBank) -> Result<(), LoadError> {
    let start_index = wad.find_lump("S_START").ok_or(LoadError::NoSprites)? + 1;
    let end_index = wad.find_lump("S_END").ok_or(LoadError::NoSprites)?;

    for idx in start_index..end_index {
        let name = Wad::lump_name_str(&wad.lumps()[idx].name);
        let id = bank.insert(name, decode_patch(name, wad.lump_bytes(idx)?))?;
        bank.register_sprite_lump(name, id);
    }

    Ok(())
}

/*====================================================================*/
/*                               Tests                                */
/*====================================================================*/
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn doom_wad() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("doom.wad")
    }

    #[test]
    fn level_and_textures_load() {
        let wad = Wad::from_file(doom_wad()).unwrap();
        let mut bank = world::TextureBank::default_with_checker();

        let marker = wad.level_indices()[0]; // E1M1
        let lvl = load_level(&wad, marker, &mut bank).expect("load");

        // simple sanity checks
        assert!(lvl.vertices.len() > 300);
        assert!(bank.len() > 1);

        let id = bank.id("STARTAN3").unwrap_or(world::NO_TEXTURE);
        let tex = bank.texture(id).unwrap();
        assert_eq!(tex.w, 128);
        assert_eq!(tex.h, 128); // STARTAN textures are 128×128
    }

    #[test]
    fn unknown_name_gets_checker() {
        let bank = world::TextureBank::default_with_checker();
        // explicitly request missing name
        let id = bank.id_or_missing("NO_SUCH_TEXTURE_XYZ");
        assert_eq!(id, 0);
    }
}
