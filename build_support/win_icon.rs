// The product icon, compiled into the Windows binaries.
//
// Explorer, the taskbar and Alt-Tab take a console program's icon from an
// `RT_GROUP_ICON`/`RT_ICON` resource inside the exe — without one,
// `docconvert.exe` shows the generic console glyph. The usual way to add
// it is an `.rc` file fed to the MSVC resource compiler, which means a
// toolchain lookup in a build script and a dependency in a crate that has
// none.
//
// So this file writes the resource itself: it turns an `.ico` into a
// `.rsrc` section and wraps that in a COFF object, which every Windows
// linker accepts as an input file. The layout follows the PE/COFF
// specification (resource directory: type → name → language → data
// entry) and is read back byte by byte in `tests/windows_icon.rs`, so a
// wrong offset fails a test on Linux before it ever reaches a linker.
//
// Included by `build.rs` (to embed the icon) and by that test (to check
// it) rather than duplicated: one implementation, two callers.

/// `RT_ICON` — one image per entry in the `.ico`.
pub const RT_ICON: u32 = 3;
/// `RT_GROUP_ICON` — the directory that lists those images.
pub const RT_GROUP_ICON: u32 = 14;

/// en-US. The language directory level of the resource tree needs a value;
/// icons are not localized, so any single one will do.
const LANG_EN_US: u32 = 0x0409;

/// Visual C++ pads resource data to 8 bytes; 4 is the documented minimum.
const DATA_ALIGNMENT: usize = 8;

const SIZE_OF_DIR_TABLE: usize = 16;
const SIZE_OF_DIR_ENTRY: usize = 8;
const SIZE_OF_DATA_ENTRY: usize = 16;
const SUBDIRECTORY: u32 = 0x8000_0000;

/// The machine a COFF object is built for. The resource section is
/// architecture-neutral, but the object header and the relocation type are
/// not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arch {
    I386,
    Amd64,
    Arm64,
}

impl Arch {
    /// Map a rustc target triple onto a COFF machine type.
    pub fn from_target(target: &str) -> Option<Arch> {
        let arch = target.split('-').next()?;
        match arch {
            "i586" | "i686" | "x86" => Some(Arch::I386),
            "x86_64" => Some(Arch::Amd64),
            "aarch64" => Some(Arch::Arm64),
            _ => None,
        }
    }

    fn machine(self) -> u16 {
        match self {
            Arch::I386 => 0x014c,
            Arch::Amd64 => 0x8664,
            Arch::Arm64 => 0xaa64,
        }
    }

    /// `IMAGE_REL_*_ADDR32NB`: a section-relative 32-bit address, which is
    /// exactly what a resource data entry holds.
    fn reloc_addr32nb(self) -> u16 {
        match self {
            Arch::I386 => 0x0007,
            Arch::Amd64 => 0x0003,
            Arch::Arm64 => 0x0002,
        }
    }
}

/// One image from the `.ico`.
pub struct Icon {
    pub width: u8,
    pub height: u8,
    pub planes: u16,
    pub bit_count: u16,
    pub data: Vec<u8>,
}

/// Parse an `.ico`: a 6-byte directory header, then one 16-byte entry per
/// image, then the images.
///
/// PNG-compressed images (the usual encoding for 256×256) are copied
/// verbatim — the resource format does not care which encoding an image
/// uses, so neither does this.
pub fn read_ico(bytes: &[u8]) -> Result<Vec<Icon>, String> {
    if bytes.len() < 6 {
        return Err("icon is shorter than its header".to_string());
    }
    let reserved = u16::from_le_bytes([bytes[0], bytes[1]]);
    let kind = u16::from_le_bytes([bytes[2], bytes[3]]);
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    if reserved != 0 || kind != 1 {
        return Err(format!("not an icon (reserved {reserved}, type {kind})"));
    }
    if count == 0 {
        return Err("icon holds no images".to_string());
    }
    if bytes.len() < 6 + count * 16 {
        return Err("icon directory is truncated".to_string());
    }
    let mut icons = Vec::with_capacity(count);
    for index in 0..count {
        let at = 6 + index * 16;
        let width = bytes[at];
        let height = bytes[at + 1];
        let planes = u16::from_le_bytes([bytes[at + 4], bytes[at + 5]]);
        let bit_count = u16::from_le_bytes([bytes[at + 6], bytes[at + 7]]);
        let size =
            u32::from_le_bytes([bytes[at + 8], bytes[at + 9], bytes[at + 10], bytes[at + 11]])
                as usize;
        let offset = u32::from_le_bytes([
            bytes[at + 12],
            bytes[at + 13],
            bytes[at + 14],
            bytes[at + 15],
        ]) as usize;
        let end = offset
            .checked_add(size)
            .ok_or_else(|| "icon image offset overflows".to_string())?;
        if end > bytes.len() {
            return Err(format!("icon image {index} runs past the end of the file"));
        }
        icons.push(Icon {
            width,
            height,
            planes: if planes == 0 { 1 } else { planes },
            bit_count: if bit_count == 0 { 32 } else { bit_count },
            data: bytes[offset..end].to_vec(),
        });
    }
    Ok(icons)
}

/// The `RT_GROUP_ICON` directory: a 6-byte header and 14 bytes per image,
/// pointing at the `RT_ICON` entries by id.
fn group_icon(icons: &[Icon]) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + icons.len() * 14);
    push16(&mut out, 0); // reserved
    push16(&mut out, 1); // type: icon
    push16(&mut out, icons.len() as u16);
    for (index, icon) in icons.iter().enumerate() {
        out.push(icon.width);
        out.push(icon.height);
        out.push(0); // palette entry count
        out.push(0); // reserved
        push16(&mut out, icon.planes);
        push16(&mut out, icon.bit_count);
        push32(&mut out, icon.data.len() as u32);
        push16(&mut out, (index + 1) as u16); // the RT_ICON id
    }
    out
}

/// Offsets of the resources we are about to write, so the directory entries
/// can point at each other.
struct Layout {
    icon_dirs: Vec<usize>,
    group_dir: usize,
    icon_data: Vec<usize>,
    group_data: usize,
    data_entries: Vec<usize>,
    total: usize,
}

fn align(offset: usize) -> usize {
    offset.next_multiple_of(DATA_ALIGNMENT)
}

/// Work out where everything goes before anything is written: the tree is
/// a fixed shape (two types, `n + 1` resources, one language each), and
/// every offset is derived from the blob sizes.
fn layout(icons: &[Icon]) -> Layout {
    let count = icons.len();
    let types = 2;
    let resources = count + 1;

    let dir_size = SIZE_OF_DIR_TABLE
        + types * SIZE_OF_DIR_ENTRY
        + (SIZE_OF_DIR_TABLE + count * SIZE_OF_DIR_ENTRY)
        + (SIZE_OF_DIR_TABLE + SIZE_OF_DIR_ENTRY)
        + resources * (SIZE_OF_DIR_TABLE + SIZE_OF_DIR_ENTRY)
        + resources * SIZE_OF_DATA_ENTRY;

    let icon_type_dir = SIZE_OF_DIR_TABLE + types * SIZE_OF_DIR_ENTRY;
    let group_type_dir = icon_type_dir + SIZE_OF_DIR_TABLE + count * SIZE_OF_DIR_ENTRY;
    let lang_dirs = group_type_dir + SIZE_OF_DIR_TABLE + SIZE_OF_DIR_ENTRY;

    let mut icon_dirs = Vec::with_capacity(count);
    for index in 0..count {
        icon_dirs.push(lang_dirs + index * (SIZE_OF_DIR_TABLE + SIZE_OF_DIR_ENTRY));
    }
    let group_dir = lang_dirs + count * (SIZE_OF_DIR_TABLE + SIZE_OF_DIR_ENTRY);

    let data_entries_at = lang_dirs + resources * (SIZE_OF_DIR_TABLE + SIZE_OF_DIR_ENTRY);
    let mut data_entries = Vec::with_capacity(resources);
    for index in 0..resources {
        data_entries.push(data_entries_at + index * SIZE_OF_DATA_ENTRY);
    }

    let mut offset = dir_size;
    let mut icon_data = Vec::with_capacity(count);
    for icon in icons {
        icon_data.push(offset);
        offset = align(offset + icon.data.len());
    }
    let group_data = offset;
    let total = align(offset + group_icon(icons).len());

    Layout {
        icon_dirs,
        group_dir,
        icon_data,
        group_data,
        data_entries,
        total,
    }
}

/// The `.rsrc` section contents: directory tree, then the data it points
/// at, all offsets relative to the start of the section.
pub fn rsrc_section(icons: &[Icon]) -> Vec<u8> {
    let layout = layout(icons);
    let count = icons.len();
    let mut out = Vec::with_capacity(layout.total);

    let icon_type_dir = SIZE_OF_DIR_TABLE + 2 * SIZE_OF_DIR_ENTRY;
    let group_type_dir = icon_type_dir + SIZE_OF_DIR_TABLE + count * SIZE_OF_DIR_ENTRY;

    // Level 1: the resource types, ids ascending.
    dir_table(&mut out, 0, 2);
    dir_entry(&mut out, RT_ICON, icon_type_dir, true);
    dir_entry(&mut out, RT_GROUP_ICON, group_type_dir, true);
    debug_assert_eq!(out.len(), icon_type_dir);

    // Level 2: one directory per type — the images, then the group.
    dir_table(&mut out, 0, count as u16);
    for (index, offset) in layout.icon_dirs.iter().enumerate() {
        dir_entry(&mut out, (index + 1) as u32, *offset, true);
    }
    debug_assert_eq!(out.len(), group_type_dir);
    dir_table(&mut out, 0, 1);
    dir_entry(&mut out, 1, layout.group_dir, true);

    // Level 3: one language directory per resource.
    for index in 0..count + 1 {
        debug_assert_eq!(
            out.len(),
            layout.lang_dir(index),
            "language directory {index} is not where the layout said"
        );
        dir_table(&mut out, 0, 1);
        dir_entry(&mut out, LANG_EN_US, layout.data_entries[index], false);
    }

    // Level 4: the data entries, addressed relative to this section.
    for (index, entry) in layout.data_entries.iter().enumerate() {
        debug_assert_eq!(out.len(), *entry);
        let (address, size) = if index < count {
            (layout.icon_data[index], icons[index].data.len())
        } else {
            (layout.group_data, group_icon(icons).len())
        };
        push32(&mut out, address as u32);
        push32(&mut out, size as u32);
        push32(&mut out, 0); // code page
        push32(&mut out, 0); // reserved
    }

    // The data itself, each blob padded so the next one starts 8-aligned.
    for icon in icons {
        out.extend_from_slice(&icon.data);
        while out.len() % DATA_ALIGNMENT != 0 {
            out.push(0);
        }
    }
    out.extend_from_slice(&group_icon(icons));
    while out.len() % DATA_ALIGNMENT != 0 {
        out.push(0);
    }
    debug_assert_eq!(out.len(), layout.total);
    out
}

impl Layout {
    /// Where the language directory for resource `index` begins — the
    /// icons first, then the group.
    fn lang_dir(&self, index: usize) -> usize {
        if index < self.icon_dirs.len() {
            self.icon_dirs[index]
        } else {
            self.group_dir
        }
    }
}

/// A COFF object with one `.rsrc` section — the form a Windows linker
/// accepts as an ordinary input file.
pub fn coff_object(icons: &[Icon], arch: Arch) -> Vec<u8> {
    let section = rsrc_section(icons);
    let layout = layout(icons);
    let relocations = (icons.len() + 1) as u16;
    let pointer_to_raw_data = 20 + 40;
    let pointer_to_relocations = pointer_to_raw_data + section.len();
    let pointer_to_symbols = pointer_to_relocations + relocations as usize * 10;

    let mut out = Vec::with_capacity(pointer_to_symbols + 18 + 4);
    // COFF file header.
    push16(&mut out, arch.machine());
    push16(&mut out, 1); // one section
    push32(&mut out, 0); // timestamp: reproducible builds
    push32(&mut out, pointer_to_symbols as u32);
    push32(&mut out, 1); // one symbol
    push16(&mut out, 0); // no optional header
    push16(&mut out, 0); // characteristics

    // Section header for `.rsrc`.
    out.extend_from_slice(b".rsrc\0\0\0");
    push32(&mut out, 0); // virtual size
    push32(&mut out, 0); // virtual address
    push32(&mut out, section.len() as u32);
    push32(&mut out, pointer_to_raw_data as u32);
    push32(&mut out, pointer_to_relocations as u32);
    push32(&mut out, 0); // no line numbers
    push16(&mut out, relocations);
    push16(&mut out, 0);
    push32(&mut out, 0x4000_0040); // initialized data, readable

    out.extend_from_slice(&section);

    // One relocation per data entry: its address field is section-relative.
    for entry in &layout.data_entries {
        push32(&mut out, *entry as u32);
        push32(&mut out, 0); // symbol 0 is the section
        push16(&mut out, arch.reloc_addr32nb());
    }

    // The section symbol, then an empty string table.
    out.extend_from_slice(b".rsrc\0\0\0");
    push32(&mut out, 0); // value
    push16(&mut out, 1); // section number
    push16(&mut out, 0); // type
    out.push(3); // IMAGE_SYM_CLASS_STATIC
    out.push(0); // no auxiliary symbols
    push32(&mut out, 4); // string table length (its own header only)

    out
}

fn dir_table(out: &mut Vec<u8>, names: u16, ids: u16) {
    push32(out, 0); // characteristics
    push32(out, 0); // timestamp
    push16(out, 0); // major version
    push16(out, 0); // minor version
    push16(out, names);
    push16(out, ids);
}

fn dir_entry(out: &mut Vec<u8>, id: u32, offset: usize, subdirectory: bool) {
    push32(out, id);
    let offset = offset as u32;
    push32(
        out,
        if subdirectory {
            offset | SUBDIRECTORY
        } else {
            offset
        },
    );
}

fn push16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
