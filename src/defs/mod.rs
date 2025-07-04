pub mod action;
pub mod flags;
pub mod mobjinfo;
pub mod sound;
pub mod state;
pub mod states;

pub use self::{
    action::Action,
    mobjinfo::{MOBJINFO, MobjInfo},
    sound::Sound,
    state::State,
    states::{STATES, StateInfo},
};

use once_cell::sync::Lazy;
use std::collections::HashMap;

static BY_DOOMEDNUM: Lazy<HashMap<u16, &'static MobjInfo>> = Lazy::new(|| {
    let mut map = HashMap::with_capacity(MOBJINFO.len());
    for info in MOBJINFO {
        if info.doomednum >= 0 {
            map.insert(info.doomednum as u16, info);
        }
    }
    map
});

pub fn by_doomednum(num: u16) -> Option<&'static MobjInfo> {
    BY_DOOMEDNUM.get(&num).copied()
}

static BY_ID: Lazy<HashMap<&'static str, &'static MobjInfo>> =
    Lazy::new(|| MOBJINFO.iter().map(|info| (info.id, info)).collect());

pub fn by_id(id: &str) -> Option<&'static MobjInfo> {
    BY_ID.get(id).copied()
}
