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

// one column entry already holds the U-coordinate (0‥tex.w-1)
// we reserve -1 to mean “already rendered”
const MASKED_DONE: i16 = -1;

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
    pub masked_mid_w: i32,
    pub z_top: f32, // front sector ceiling world-Z
    pub z_bot: f32, // front sector floor   world-Z

    // per-column *flag* slice:
    //   >=0  – u already filled by wall loop
    //   -1   – column was rendered during sprite pass
    pub masked_cols: Range<usize>,

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
        &mut self,
        seg_idx: SegmentId,
        edge: &Edge,
        z_top: f32,
        z_bot: f32,
        masked_mid: TextureId,
        texture_bank: &TextureBank,
    ) -> DrawSeg {
        let scale1 = self.focal * edge.invz_l;
        let scale2 = self.focal * edge.invz_r;
        let scale_step = (scale2 - scale1) / ((edge.x_r - edge.x_l) as f32);
        let count = (edge.x_r - edge.x_l + 1) as usize;

        let masked_mid_w = if masked_mid != NO_TEXTURE {
            texture_bank.texture(masked_mid).unwrap().w as i32
        } else {
            0
        };

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
            masked_mid_w,
            z_top,
            z_bot,
            masked_cols: self.frame_scratch.alloc(count),
            top_clip: self.frame_scratch.alloc(count),
            bot_clip: self.frame_scratch.alloc(count),
        }
    }

    pub fn store_wall_range(&mut self, ds: &mut DrawSeg, col: usize, uoz_invz: i32) {
        let idx = col - ds.x1 as usize;

        debug_assert!(idx < ds.masked_cols.len());

        if ds.silhouette.contains(Silhouette::TOP) {
            self.frame_scratch.openings[ds.top_clip.start + idx] = self.clip_bands.ceil[col];
        }

        if ds.silhouette.contains(Silhouette::BOTTOM) {
            self.frame_scratch.openings[ds.bot_clip.start + idx] = self.clip_bands.floor[col];
        }

        if ds.masked_mid != NO_TEXTURE {
            self.frame_scratch.openings[ds.masked_cols.start + idx] =
                uoz_invz.rem_euclid(ds.masked_mid_w) as i16;
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

    pub fn draw_sprites(&mut self, level: &Level, tex: &TextureBank) {
        let focal = self.focal;
        let h_scr = self.height as i32;

        for i in 0..self.sprites.len() {
            let vis = self.sprites[i]; // copy: no borrow lives
            let tex_spr = tex.texture(vis.tex).unwrap();
            let spr_scale = focal * vis.invz;

            let mut x = vis.x0.max(0);
            let x_end = vis.x1.min(self.width as i32 - 1);
            let x_clip_left = x - vis.x0; // how many columns we skipped
            let mut u_acc = x_clip_left as f32 * vis.u_step;

            while x <= x_end {
                let (ceil, floor) = self.column_clips(level, spr_scale, &vis, x, tex);

                if ceil >= floor {
                    u_acc += vis.u_step;
                    x += 1;
                    continue;
                }

                // intersect with sprite’s own Y span
                let y0 = ceil.max(vis.y0).max(0);
                let y1 = floor.min(vis.y1).min(h_scr - 1);

                let u = u_acc as usize;
                if u >= tex_spr.w {
                    break;
                }

                let v_step = tex_spr.h as f32 / (vis.y1 - vis.y0 + 1) as f32;
                let mut v_acc = (y0 - vis.y0) as f32 * v_step;

                for y in y0..=y1 {
                    let v = (v_acc as usize).min(tex_spr.h - 1);
                    let idx = tex_spr.pixels[v * tex_spr.w + u];
                    if idx != 0 {
                        self.scratch[y as usize * self.width + x as usize] = tex.get_color(0, idx);
                    }
                    v_acc += v_step;
                }

                u_acc += vis.u_step;
                x += 1;
            }
        }

        // second pass: any masked mids not yet drawn
        for ds_idx in (0..self.drawsegs.len()).rev() {
            if self.drawsegs[ds_idx].masked_mid != NO_TEXTURE {
                let ds = &self.drawsegs[ds_idx];
                self.render_masked_seg_range(ds_idx, ds.x1, ds.x2, tex);
            }
        }
    }

    fn column_clips(
        &mut self,
        level: &Level,
        spr_scale: f32,
        vis: &VisSprite,
        x: i32,
        tex: &TextureBank,
    ) -> (i32, i32) {
        let mut ceil = -1;
        let mut floor = self.height as i32;

        for ds_idx in (0..self.drawsegs.len()).rev() {
            let (behind, masked, t_idx, b_idx) = {
                let ds = &self.drawsegs[ds_idx];
                if x < ds.x1 || x > ds.x2 {
                    continue;
                }

                let max = ds.scale1.max(ds.scale2);
                let min = ds.scale1.min(ds.scale2);
                let back = if max < spr_scale {
                    true
                } else if min < spr_scale {
                    Self::point_on_seg_backside(level, vis.gx, vis.gy, ds.cur_line)
                } else {
                    false
                };

                (
                    back,
                    ds.masked_mid != NO_TEXTURE,
                    ds.silhouette
                        .contains(Silhouette::TOP)
                        .then(|| ds.top_clip.start + (x - ds.x1) as usize),
                    ds.silhouette
                        .contains(Silhouette::BOTTOM)
                        .then(|| ds.bot_clip.start + (x - ds.x1) as usize),
                )
            }; // borrow ends here

            if behind {
                if masked {
                    self.render_masked_seg_range(ds_idx, x, x, tex);
                }
                continue;
            }

            if let Some(i) = t_idx {
                ceil = ceil.max(self.frame_scratch.openings[i] as i32);
            }
            if let Some(i) = b_idx {
                floor = floor.min(self.frame_scratch.openings[i] as i32);
            }

            if ceil >= floor {
                break;
            }
        }

        (ceil, floor)
    }

    fn render_masked_seg_range(&mut self, ds_idx: usize, x0: i32, x1: i32, tex_bank: &TextureBank) {
        let ds = &self.drawsegs[ds_idx];
        let openings = &mut self.frame_scratch.openings;
        let tex_mid = tex_bank.texture(ds.masked_mid).unwrap();

        // ------------------------------------------------------------------
        // vertical stepping
        // ------------------------------------------------------------------
        let mut scale = ds.scale1 + (x0 - ds.x1) as f32 * ds.scale_step;

        for x in x0..=x1 {
            let col = (x - ds.x1) as usize;
            let ds_top_clip = openings[ds.top_clip.start + col] as i32 + 1;
            let ds_bot_clip = openings[ds.bot_clip.start + col] as i32 - 1;
            let entry = &mut openings[ds.masked_cols.start + col];
            if *entry == MASKED_DONE {
                scale += ds.scale_step;
                continue; // already rendered
            }

            // integer texel column
            let u = *entry as usize; // 0 … tex_mid.w-1

            // ------- project vertical extents --------------------------------
            let y_top = (self.half_h as f32 - (ds.z_top - self.view_z) * scale).floor() as i32;
            let y_bot = (self.half_h as f32 - (ds.z_bot - self.view_z) * scale).ceil() as i32;

            let mut y0 = y_top.max(0);
            let mut y1 = y_bot.min(self.height as i32 - 1);

            if ds.silhouette.contains(Silhouette::TOP) {
                y0 = y0.max(ds_top_clip);
            }
            if ds.silhouette.contains(Silhouette::BOTTOM) {
                y1 = y1.min(ds_bot_clip);
            }

            // ------- draw the column ----------------------------------------
            if y0 <= y1 {
                let v_step = tex_mid.h as f32 / (y_bot - y_top + 1) as f32;
                let mut v_f = (y0 - y_top) as f32 * v_step;

                for y in y0..=y1 {
                    let v = (v_f as usize).min(tex_mid.h - 1);
                    let idx = tex_mid.pixels[v * tex_mid.w + u];
                    if idx != 0 {
                        self.scratch[y as usize * self.width + x as usize] =
                            tex_bank.get_color(0, idx);
                    }
                    v_f += v_step;
                }
            }

            *entry = MASKED_DONE; // mark drawn
            scale += ds.scale_step;
        }
    }

    fn point_on_seg_backside(level: &Level, px: f32, py: f32, seg_id: SegmentId) -> bool {
        let seg = &level.segs[seg_id as usize];
        let v1 = &level.vertices[seg.v1 as usize].pos;
        let v2 = &level.vertices[seg.v2 as usize].pos;

        // Doom’s exact R_PointOnSegSide test:
        //   back side (dy * dx1  -  dx * dy1) > 0
        let dx = v2.x - v1.x;
        let dy = v2.y - v1.y;
        let dx1 = px - v1.x;
        let dy1 = py - v1.y;

        (dy * dx1 - dx * dy1) > 0.0 // true  == sprite is on back side
    }
}
