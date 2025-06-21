use glam::Vec2;

use crate::{
    engine::planes::PlaneMap,
    engine::types::{ClipRange, Screen, Viewer},
    renderer::{ClipBands, Renderer, Rgba},
    world::{
        bsp::{CHILD_MASK, SUBSECTOR_BIT},
        camera::Camera,
        geometry::Level,
        texture::TextureBank,
    },
};

pub struct Engine<R: Renderer> {
    pub renderer: R,
    pub level: Level,
    pub camera: Camera,
    pub texture_bank: TextureBank,
    pub clip_bands: ClipBands,
    pub screen: Screen,
    pub view: Viewer,
    pub visplane_map: PlaneMap,
    pub solid_segs: Vec<ClipRange>,
}

impl<R: Renderer> Engine<R> {
    pub fn new(
        renderer: R,
        level: Level,
        camera: Camera,
        texture_bank: TextureBank,
        w: usize,
        h: usize,
    ) -> Self {
        let half_w = w as f32 * 0.5;
        let half_h = h as f32 * 0.5;

        Self {
            renderer,
            level,
            camera,
            texture_bank,
            clip_bands: ClipBands {
                ceil: vec![i16::MIN; w],
                floor: vec![i16::MAX; w],
            },
            screen: Screen {
                w,
                h,
                half_w,
                half_h,
            },
            view: Viewer::default(),
            visplane_map: PlaneMap::new(w),
            solid_segs: Vec::new(),
        }
    }

    pub fn render_frame(&mut self, mut submit: impl FnMut(&[Rgba], usize, usize)) {
        self.renderer.begin_frame(self.screen.w, self.screen.h);

        // fully open clips at start of frame
        self.clip_bands.ceil.fill(i16::MIN);
        self.clip_bands.floor.fill(i16::MAX);

        self.init_solid_segs();

        self.view.focal = self.camera.screen_scale(self.screen.w);
        self.view.floor_z =
            Self::floor_height_under_player(&self.level, self.camera.pos().truncate());
        self.view.view_z = self.view.floor_z + self.camera.pos().z;

        self.visplane_map.clear();

        self.walk_bsp(self.level.bsp_root(), &mut submit);

        self.visplane_map.draw_all(
            &mut self.renderer,
            &self.level,
            &self.camera,
            &self.screen,
            &self.view,
            &self.texture_bank,
        );

        self.renderer.end_frame(submit);
    }

    fn walk_bsp(&mut self, child: u16, submit: &mut impl FnMut(&[Rgba], usize, usize)) {
        if child & SUBSECTOR_BIT != 0 {
            self.draw_subsector(child & CHILD_MASK, submit);
            return;
        }

        // Internal node ──────
        let node = &self.level.nodes[child as usize];
        let front = node.point_side(self.camera.pos().truncate()) as usize; // 0: front, 1: back
        let near = node.child[front];
        let back = node.child[front ^ 1];
        let back_visible = self.bbox_visible(&node.bbox[front ^ 1]);

        // Near side first …
        self.walk_bsp(near, submit);

        // … far side only if its bounding box might be visible.
        if back_visible {
            self.walk_bsp(back, submit);
        }
    }

    /// Return the floor height (Z) of the sector the player is currently in.
    fn floor_height_under_player(level: &Level, pos: Vec2) -> f32 {
        let ss_idx = Self::find_subsector(level, pos);
        let ss = &level.subsectors[ss_idx];
        let seg = &level.segs[ss.first_seg as usize];
        let ld = &level.linedefs[seg.linedef as usize];
        let sd_idx = if seg.dir == 0 {
            ld.right_sidedef
        } else {
            ld.left_sidedef
        }
        .expect("subsector SEG must have a sidedef");
        let sector = &level.sectors[level.sidedefs[sd_idx as usize].sector as usize];
        sector.floor_h as f32
    }

    /// Walk the BSP until we hit a subsector leaf that contains `pos`.
    fn find_subsector(level: &Level, pos: Vec2) -> usize {
        let mut idx = level.bsp_root() as u16;
        loop {
            if idx & SUBSECTOR_BIT != 0 {
                return (idx & CHILD_MASK) as usize;
            }
            let node = &level.nodes[idx as usize];
            let side = node.point_side(pos) as usize;
            idx = node.child[side];
        }
    }
}
