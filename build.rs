//! The game's icon in the Windows executable, for Explorer, the desktop
//! and a pinned task bar entry (the window's own icon is set at run time,
//! `window::window_icon`).
//!
//! `assets/icon/washoi.ico` becomes a compiled resource file (`.res`: one
//! `RT_ICON` per image and the `RT_GROUP_ICON` that lists them, as
//! `1 ICON "washoi.ico"` in an `.rc` file compiles to), which the MSVC
//! linker takes as an input. Written here rather than with a resource
//! compiler, so the build needs no other tool. Other targets need nothing.

use std::env;
use std::fs;
use std::path::PathBuf;

const ICON: &str = "assets/icon/washoi.ico";
/// `RT_ICON` and `RT_GROUP_ICON`.
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
/// `MOVEABLE | PURE | DISCARDABLE`, as resource compilers mark icons.
const ICON_FLAGS: u16 = 0x1010;
const GROUP_FLAGS: u16 = 0x1030;
/// English (United States), the resource compilers' default.
const LANGUAGE: u16 = 0x0409;

fn main() {
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed=build.rs");
    let windows = env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|e| e == "msvc");
    if !(windows && msvc) {
        return;
    }
    let ico = fs::read(ICON).expect("the icon");
    let res = icon_res(&ico).expect("a valid .ico");
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("washoi.res");
    fs::write(&out, res).expect("the resource file");
    println!("cargo:rustc-link-arg-bins={}", out.display());
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// One resource entry: its header (numeric type and name), its data, and
/// the padding to a four-byte boundary.
fn entry(out: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&[0xff, 0xff]);
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&[0xff, 0xff]);
    out.extend_from_slice(&name.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&LANGUAGE.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // Version
    out.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

/// The `.res` for an `.ico`: its images as icons 1 to n, and group 1.
fn icon_res(ico: &[u8]) -> Option<Vec<u8>> {
    if u16_at(ico, 0)? != 0 || u16_at(ico, 2)? != 1 {
        return None;
    }
    let count = u16_at(ico, 4)?;
    let mut res = Vec::new();
    // The empty entry every .res file starts with.
    res.extend_from_slice(&[0, 0, 0, 0, 0x20, 0, 0, 0, 0xff, 0xff, 0, 0, 0xff, 0xff, 0, 0]);
    res.extend_from_slice(&[0; 16]);
    let mut group = Vec::new();
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&count.to_le_bytes());
    for i in 0..count {
        let at = 6 + 16 * usize::from(i);
        let header = ico.get(at..at + 12)?;
        let size = u32_at(ico, at + 8)? as usize;
        let offset = u32_at(ico, at + 12)? as usize;
        let image = ico.get(offset..offset + size)?;
        entry(&mut res, RT_ICON, i + 1, ICON_FLAGS, image);
        // GRPICONDIRENTRY: the ICONDIRENTRY with the icon's id for its
        // offset, and one plane where the file says none (as `rc` does).
        group.extend_from_slice(&header[..4]);
        group.extend_from_slice(&u16_at(header, 4)?.max(1).to_le_bytes());
        group.extend_from_slice(&header[6..]);
        group.extend_from_slice(&(i + 1).to_le_bytes());
    }
    entry(&mut res, RT_GROUP_ICON, 1, GROUP_FLAGS, &group);
    Some(res)
}
