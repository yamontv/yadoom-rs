// Format-agnostic repository of textures decoded by the asset loader.
// The renderer and world logic interact through `TextureId` only.

use std::collections::HashMap;

/// Runtime handle for a texture in this bank.
///
/// *Guaranteed* to remain stable for the lifetime of the bank.
pub type TextureId = u16;

/// `TextureId` whose pixels are the checkerboard fallback.
/// Always = 0 because `TextureBank::new()` inserts it first.
pub const NO_TEXTURE: TextureId = 0;

/// CPU-side storage: 32-bit **ARGB**  (0xAARRGGBB) in row-major order.
/// The loader fills the pixel vector; the renderer may later upload it
/// to the GPU and drop the CPU copy if desired.
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub w: usize,
    pub h: usize,
    pub pixels: Vec<u32>,
}

/// Things that can go wrong when using the bank.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TextureError {
    /// Attempted to insert a second texture with an existing name.
    #[error("texture name `{0}` already present in bank")]
    Duplicate(String),

    /// Requested ID is outside `0 .. bank.len()`.
    #[error("texture id {0} out of range")]
    BadId(TextureId),
}

/// A palette-agnostic, format-agnostic cache of textures.
///
/// * Does **not** know about WADs, PNG, OpenGL — that’s the loader’s job.
/// * Stores exactly one copy of every name.
/// * ID **0** is always the “missing” checkerboard.
///
/// **Thread-safety:** access `TextureBank` from a single thread or wrap it
/// in `RwLock`; the struct itself is not `Sync`.
pub struct TextureBank {
    by_name: HashMap<String, TextureId>,
    data: Vec<Texture>,
}

impl TextureBank {
    // ---------------------------------------------------------------------
    // Constructors
    // ---------------------------------------------------------------------

    /// Create an empty bank with a mandatory *missing* texture used as
    /// fallback.  The texture is inserted under the fixed name `"MISSING"`
    /// and obtains the handle **0**.
    pub fn new(missing_tex: Texture) -> Self {
        let mut by_name = HashMap::new();
        by_name.insert("MISSING".into(), NO_TEXTURE);
        Self {
            by_name,
            data: vec![missing_tex],
        }
    }

    /// Convenience checkerboard 8×8 (dark/light grey).
    pub fn default_with_checker() -> Self {
        let mut pix = vec![0u32; 8 * 8];
        for y in 0..8 {
            for x in 0..8 {
                pix[y * 8 + x] = if (x ^ y) & 1 == 0 {
                    0xFF_909090
                } else {
                    0xFF_303030
                };
            }
        }
        Self::new(Texture {
            w: 8,
            h: 8,
            pixels: pix,
        })
    }

    // ---------------------------------------------------------------------
    // Query helpers
    // ---------------------------------------------------------------------

    /// Number of textures stored (including the “missing” one).
    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.data.len() == 1
    } // only checker

    /// Obtain the id for a *loaded* texture by name.
    /// Returns `None` if the name is unknown.
    pub fn id(&self, name: &str) -> Option<TextureId> {
        self.by_name.get(name).copied()
    }

    /// Fallback-safe query: unknown names resolve to the checkerboard id.
    pub fn id_or_missing(&self, name: &str) -> TextureId {
        self.id(name).unwrap_or(NO_TEXTURE)
    }

    /// Borrow a texture by id, with bounds-checking.
    pub fn texture(&self, id: TextureId) -> Result<&Texture, TextureError> {
        self.data.get(id as usize).ok_or(TextureError::BadId(id))
    }

    /// Mutable borrow (e.g. for post-load mip-generation).
    pub fn texture_mut(&mut self, id: TextureId) -> Result<&mut Texture, TextureError> {
        self.data
            .get_mut(id as usize)
            .ok_or(TextureError::BadId(id))
    }

    // ---------------------------------------------------------------------
    // Mutations
    // ---------------------------------------------------------------------

    /// Insert a texture under `name`.
    ///
    /// * Returns the newly assigned `TextureId`.
    /// * Fails if the name already exists (`Duplicate`).
    pub fn insert<S: Into<String>>(
        &mut self,
        name: S,
        tex: Texture,
    ) -> Result<TextureId, TextureError> {
        let name = name.into();
        if self.by_name.contains_key(&name) {
            return Err(TextureError::Duplicate(name));
        }
        let id = self.data.len() as TextureId;
        self.data.push(tex);
        self.by_name.insert(name, id);
        Ok(id)
    }
}

/*======================================================================*/
/*                               Tests                                  */
/*======================================================================*/
#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_tex(color: u32) -> Texture {
        Texture {
            w: 2,
            h: 2,
            pixels: vec![color; 4],
        }
    }

    #[test]
    fn insert_and_lookup() {
        let mut bank = TextureBank::default_with_checker();
        let red = bank.insert("RED", dummy_tex(0xFF_FF0000)).unwrap();
        let blue = bank.insert("BLUE", dummy_tex(0xFF_0000FF)).unwrap();

        assert_ne!(red, NO_TEXTURE);
        assert_ne!(blue, red);
        assert_eq!(bank.id("RED"), Some(red));
        assert_eq!(bank.id("BLUE"), Some(blue));
        assert_eq!(bank.id("NOPE"), None);

        assert_eq!(bank.texture(red).unwrap().pixels[0], 0xFF_FF0000);
        assert_eq!(bank.texture(blue).unwrap().pixels[0], 0xFF_0000FF);
    }

    #[test]
    fn duplicate_name_rejected() {
        let mut bank = TextureBank::default_with_checker();
        bank.insert("WOOD", dummy_tex(1)).unwrap();
        let err = bank.insert("WOOD", dummy_tex(2)).unwrap_err();
        assert_eq!(err, TextureError::Duplicate("WOOD".into()));
        // texture count still 2 (checker + first WOOD)
        assert_eq!(bank.len(), 2);
    }

    #[test]
    fn bad_id_guard() {
        let bank = TextureBank::default_with_checker();
        let bad = TextureId::MAX;
        assert_eq!(bank.texture(bad).unwrap_err(), TextureError::BadId(bad));
    }
}
