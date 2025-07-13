use bitflags::bitflags;

bitflags! {
    /// Behaviour / collision flags carried by every **mobj** at runtime.
    ///
    /// The numeric values are copied 1-for-1 from `doom/info.h`.
    #[derive(Default, Clone, Copy, Debug)]
    pub struct MobjFlags: u32 {
        // Call `P_SpecialThing` when touched.
        const SPECIAL        = 0x0000_0001;
        // Blocks movement.
        const SOLID          = 0x0000_0002;
        // Can be hit by bullets/projectiles.
        const SHOOTABLE      = 0x0000_0004;
        // Invisible to PVS links, still touchable.
        const NOSECTOR       = 0x0000_0008;
        // Removed from blockmap, still rendered.
        const NOBLOCKMAP     = 0x0000_0010;

        // AI / spawn modifiers
        const AMBUSH         = 0x0000_0020;
        const JUSTHIT        = 0x0000_0040;
        const JUSTATTACKED   = 0x0000_0080;
        const SPAWNCEILING   = 0x0000_0100;
        const NOGRAVITY      = 0x0000_0200;

        // Movement-related
        const DROPOFF        = 0x0000_0400;
        const PICKUP         = 0x0000_0800;
        const NOCLIP         = 0x0000_1000;
        const SLIDE          = 0x0000_2000;
        const FLOAT          = 0x0000_4000;
        const TELEPORT       = 0x0000_8000;

        // Projectiles / drops
        const MISSILE        = 0x0001_0000;
        const DROPPED        = 0x0002_0000;

        // Rendering / damage tweaks
        const SHADOW         = 0x0004_0000;
        const NOBLOOD        = 0x0008_0000;
        const CORPSE         = 0x0010_0000;
        const INFLOAT        = 0x0020_0000;

        // Inter-mission counters
        const COUNTKILL      = 0x0040_0000;
        const COUNTITEM      = 0x0080_0000;

        // Special cases
        const SKULLFLY       = 0x0100_0000;
        const NOTDMATCH      = 0x0200_0000;

        // Upper two bits encode multiplayer palette translation.
        const TRANSLATION    = 0x0C00_0000;
    }
}

/// Bit-shift used to extract the player-colour translation (0‥3) from
/// the upper bits of `MobjFlags::TRANSLATION`.
pub const MF_TRANSSHIFT: u32 = 26;
