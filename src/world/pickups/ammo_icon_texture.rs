//! wash-fps `world/pickups/AmmoIconTexture.ts`: the Warsow simpleitem icon an
//! ammo crate wears on its sides.

use shagger::loader::pk3::pk3_loader::Pk3Loader;
use shagger::loader::texture::{DecodedTexture, resolve_and_load_texture};

/// `loadAmmoIconTexture`: the icon by its shader or image name, else the
/// loose `.tga`.
pub fn load_ammo_icon_texture(pk3: &mut Pk3Loader, icon_path: &str) -> Option<DecodedTexture> {
    resolve_and_load_texture(pk3, icon_path).or_else(|| resolve_and_load_texture(pk3, &format!("{icon_path}.tga")))
}
