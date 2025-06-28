use std::ops::Range;

use crate::{
    renderer::software::{Software, projection::Edge},
    world::{
        camera::Camera,
        geometry::{Level, SegmentId, SubsectorId},
        texture::{NO_TEXTURE, TextureBank, TextureId},
    },
};
use bitflags::bitflags;

#[derive(Default)]
pub struct FrameScratch {
    openings: Vec<i16>,
    cursor: usize,
}
impl FrameScratch {
    /// Allocate `len` consecutive i16 slots inside `openings`
    /// and return the index range that was handed out.
    pub fn alloc(&mut self, len: usize) -> Range<usize> {
        let start = self.cursor;
        self.cursor += len;

        if self.cursor > self.openings.len() {
            self.openings.resize(self.cursor.next_power_of_two(), 0);
        }
        start..start + len
    }

    pub fn reset(&mut self) {
        self.cursor = 0;
    }
}

#[derive(Clone, Default)]
pub struct DrawSeg {
    pub cur_line: SegmentId,
    pub x1: i32,
    pub x2: i32,

    pub scale1: f32,
    pub scale2: f32,
    pub scale_step: f32,

    pub silhouette: Silhouette,
    pub bsil_height: f32, // do not clip sprites above this
    pub tsil_height: f32, // do not clip sprites below this

    pub masked_mid: TextureId,

    /// Pointers into the global `openings` pool (or empty slices if not needed)
    pub top_clip: Range<usize>,
    pub bot_clip: Range<usize>,
}

bitflags! {
    #[derive(Default, Clone)]
    pub struct Silhouette: u8 {
        const NONE = 0x0000;
        const BOTTOM = 0x0001;
        const TOP    = 0x0002;
        const SOLID  = 0x0003;
    }
}

#[derive(Clone, Copy)]
pub struct VisSprite {
    pub x0: i32, // inclusive
    pub x1: i32, // inclusive
    pub y0: i32,
    pub y1: i32,
    pub invz: f32, // 1 / camera-space Y (depth)
    pub gx: f32,   // world X  (for side test)
    pub gy: f32,   // world Y
    pub tex: TextureId,
    pub u_step: f32, // how far to advance U per screen pixel X
}

/// very small subset just to get moving
const THING_SPRITE: &[(u16, &str)] = &[
    (1, "PLAYA1"),    // player 1 start
    (2014, "BON1A0"), // BON
    (3001, "TROOA0"), // imp (front-facing, no rotation)
    (3004, "POSSA0"), // zombieman
    (2004, "CLIPA0"), // clip pickup
];

fn sprite_for(type_id: u16, tex_bank: &TextureBank) -> TextureId {
    THING_SPRITE
        .iter()
        .find(|(id, _)| *id == type_id)
        .and_then(|(_, lump)| tex_bank.id(*lump))
        .unwrap_or(NO_TEXTURE)
}

impl Software {
    pub fn create_draw_seg(
        &self,
        seg_idx: SegmentId,
        edge: &Edge,
        masked_mid: TextureId,
    ) -> DrawSeg {
        let scale1 = self.focal * edge.invz_l;
        let scale2 = self.focal * edge.invz_r;
        let scale_step = (scale2 - scale1) / ((edge.x_r - edge.x_l) as f32);

        DrawSeg {
            cur_line: seg_idx,
            x1: edge.x_l,
            x2: edge.x_r,
            scale1,
            scale2,
            scale_step,
            silhouette: Silhouette::NONE,
            bsil_height: f32::MIN,
            tsil_height: f32::MAX,
            masked_mid: masked_mid,
            top_clip: 0..0,
            bot_clip: 0..0,
        }
    }

    pub fn update_draw_seg_clips(&mut self, ds: &mut DrawSeg) {
        let count = (ds.x2 - ds.x1 + 1) as usize;

        if ds.silhouette.contains(Silhouette::TOP) {
            let range = self.frame_scratch.alloc(count);
            for i in 0..count {
                self.frame_scratch.openings[range.start + i] =
                    self.clip_bands.ceil[ds.x1 as usize + i];
            }
            ds.top_clip = range;
        } else {
            ds.top_clip = 0..0;
        }

        if ds.silhouette.contains(Silhouette::BOTTOM) {
            let range = self.frame_scratch.alloc(count);
            for i in 0..count {
                self.frame_scratch.openings[range.start + i] =
                    self.clip_bands.floor[ds.x1 as usize + i];
            }
            ds.bot_clip = range;
        } else {
            ds.bot_clip = 0..0;
        }
    }

    pub fn collect_sprites_for_subsector(
        &mut self,
        ss_idx: SubsectorId,
        level: &Level,
        camera: &Camera,
        tex_bank: &TextureBank,
    ) {
        let mut out: Vec<VisSprite> = Vec::new();
        let focal = camera.screen_scale(self.width);
        let half_w = self.half_w;
        let half_h = self.half_h;

        let sec_idx = level.subsectors[ss_idx as usize].sector;
        let floor_z = level.sectors[sec_idx as usize].floor_h as f32;

        for thing_idx in level.subsectors[ss_idx as usize].things.iter() {
            let thing = &level.things[*thing_idx as usize];

            let tex_id = sprite_for(thing.type_id, tex_bank);

            if tex_id == NO_TEXTURE {
                continue;
            }

            // camera space -------------------------------------------------
            let rel = camera.to_cam(&thing.pos); // z=0 floor aligned
            if rel.y <= 4.0 {
                // “behind” or too close to near-plane
                continue;
            }
            let invz = 1.0 / rel.y;
            let scale = focal * invz;

            let tex = tex_bank.texture(tex_id).unwrap();
            let sprite_w = tex.w as f32 * scale;
            let sprite_h = tex.h as f32 * scale;

            let xc = half_w + rel.x * scale;
            let x0 = (xc - sprite_w * 0.5).floor() as i32;
            let x1 = (xc + sprite_w * 0.5).ceil() as i32;

            if x1 < 0 || x0 >= self.width as i32 {
                continue; // completely off-screen
            }

            // vertical offset between sprite base (sector floor) and the eye
            let rel_z = floor_z - self.view_z;

            let y_bottom = half_h - rel_z * scale;

            let y0 = (y_bottom - sprite_h).floor() as i32; // top
            let y1 = (y_bottom).ceil() as i32; // bottom (touching floor)

            out.push(VisSprite {
                x0,
                x1,
                y0,
                y1,
                invz,
                gx: thing.pos.x,
                gy: thing.pos.y,
                tex: tex_id,
                u_step: tex.w as f32 / (x1 - x0 + 1) as f32,
            });
        }

        // far-to-near painter’s algorithm so we overdraw correctly
        out.sort_by(|a, b| b.invz.partial_cmp(&a.invz).unwrap());
        self.sprites.append(&mut out);
    }

    pub fn draw_sprites(&mut self, level: &Level, tex_bank: &TextureBank) {
        let openings = &self.frame_scratch.openings; // clip rows arena
        let focal = self.focal; // already stored in Software

        for spr in &self.sprites {
            // Sprite's “scale” in the same metric the walls use
            let spr_scale = focal * spr.invz;

            let tex = tex_bank.texture(spr.tex).unwrap();
            let u_inc = spr.u_step;

            let mut x = spr.x0.max(0);
            let x_end = spr.x1.min(self.width as i32 - 1);
            let mut u_f = 0.0_f32;

            // ------------------------------------------------ column loop ----
            while x <= x_end {
                // --------- clip bands built from nearer drawsegs ------------
                let mut ceil = -1; // fully open above
                let mut floor = self.height as i32; // fully open below

                // drawsegs were pushed back-to-front ⇒ walk in reverse
                for ds in self.drawsegs.iter().rev() {
                    // seg does not touch this column
                    if x < ds.x1 || x > ds.x2 {
                        continue;
                    }

                    let max_scale = ds.scale1.max(ds.scale2);
                    let low_scale = ds.scale1.min(ds.scale2);

                    let seg_is_behind = if max_scale < spr_scale {
                        true // both edges farther
                    } else if low_scale < spr_scale {
                        // one edge closer, one edge farther → need side test
                        Self::point_on_seg_backside(level, spr.gx, spr.gy, ds.cur_line)
                    } else {
                        false // unquestionably in front
                    };
                    if seg_is_behind {
                        continue;
                    }

                    // TOP silhouette
                    if ds.silhouette.contains(Silhouette::TOP) {
                        let idx = ds.top_clip.start + (x - ds.x1) as usize;
                        ceil = ceil.max(openings[idx] as i32);
                    }
                    // BOTTOM silhouette
                    if ds.silhouette.contains(Silhouette::BOTTOM) {
                        let idx = ds.bot_clip.start + (x - ds.x1) as usize;
                        floor = floor.min(openings[idx] as i32);
                    }

                    if ceil >= floor {
                        break; // sprite column fully hidden
                    }
                }

                if ceil >= floor {
                    u_f += u_inc;
                    x += 1;
                    continue; // nothing visible in this column
                }

                // ---------------- draw the sprite column -------------------
                let u = u_f as usize;
                if u >= tex.w {
                    break;
                }

                let y0_clip = ceil.max(spr.y0).max(0);
                let y1_clip = floor.min(spr.y1).min(self.height as i32 - 1);

                let v_step = tex.h as f32 / (spr.y1 - spr.y0 + 1) as f32;
                let mut v_f = (y0_clip - spr.y0) as f32 * v_step;

                for y in y0_clip..=y1_clip {
                    let v = v_f as usize;
                    if v >= tex.h {
                        break;
                    }
                    let idx = tex.pixels[v * tex.w + u];
                    if idx != 0 {
                        self.scratch[y as usize * self.width + x as usize] =
                            tex_bank.get_color(0, idx);
                    }
                    v_f += v_step;
                }

                u_f += u_inc;
                x += 1;
            }
        }
    }

    fn point_on_seg_backside(level: &Level, px: f32, py: f32, seg_id: SegmentId) -> bool {
        let seg = &level.segs[seg_id as usize];
        let v1 = &level.vertices[seg.v1 as usize];
        let v2 = &level.vertices[seg.v2 as usize];
        ((px - v1.pos.x) * (v2.pos.y - v1.pos.y) - (py - v1.pos.y) * (v2.pos.x - v1.pos.x)) < 0.0
    }
}
