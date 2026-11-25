//! The icon that the build embeds, checked against the PE/COFF
//! specification rather than against the code that wrote it: the section is
//! parsed from scratch here — type table, name tables, language tables, data
//! entries — and every offset is followed, so a wrong layout fails on Linux
//! long before a Windows linker ever sees it.

#![allow(dead_code)]

include!("../build_support/win_icon.rs");

const ICON: &[u8] = include_bytes!("../assets/docconvert.ico");

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// One entry of a resource directory table.
struct Entry {
    id: u32,
    offset: usize,
    is_directory: bool,
}

fn directory(bytes: &[u8], at: usize) -> (Vec<Entry>, usize) {
    assert!(at + 16 <= bytes.len(), "directory at {at} is out of range");
    let named = le16(bytes, at + 12) as usize;
    let ids = le16(bytes, at + 14) as usize;
    let mut entries = Vec::with_capacity(named + ids);
    for index in 0..named + ids {
        let entry_at = at + 16 + index * 8;
        let id = le32(bytes, entry_at);
        let raw = le32(bytes, entry_at + 4);
        entries.push(Entry {
            id,
            offset: (raw & 0x7fff_ffff) as usize,
            is_directory: raw & 0x8000_0000 != 0,
        });
    }
    (entries, at + 16 + (named + ids) * 8)
}

#[test]
fn the_icon_file_is_the_one_that_ships() {
    let icons = read_ico(ICON).expect("the committed icon parses");
    assert!(icons.len() >= 4, "{} images", icons.len());
    // Big to small, as Windows prefers, and one 16x16 for the tree view.
    assert_eq!(icons[0].width, 0, "a 256x256 entry comes first");
    assert!(icons.iter().any(|icon| icon.width == 16), "no 16x16 entry");
    for icon in &icons {
        assert!(!icon.data.is_empty(), "an empty image");
        assert_eq!(icon.planes, 1);
        assert_eq!(icon.bit_count, 32);
    }
}

#[test]
fn the_resource_section_resolves_to_the_icon() {
    let icons = read_ico(ICON).expect("the committed icon parses");
    let section = rsrc_section(&icons);
    let count = icons.len();

    // Level 1: exactly the two types this build uses, ids ascending.
    let (types, after) = directory(&section, 0);
    assert_eq!(types.len(), 2, "two resource types");
    assert_eq!(types[0].id, RT_ICON);
    assert_eq!(types[1].id, RT_GROUP_ICON);
    assert!(types.iter().all(|entry| entry.is_directory));
    assert_eq!(after, types[0].offset, "the tables follow the type table");

    // Level 2: one resource per image, ids 1..n, then the group.
    let (images, after_images) = directory(&section, types[0].offset);
    assert_eq!(images.len(), count);
    for (index, entry) in images.iter().enumerate() {
        assert_eq!(entry.id, index as u32 + 1, "image ids are 1-based");
        assert!(entry.is_directory);
    }
    assert_eq!(after_images, types[1].offset);
    let (groups, after_groups) = directory(&section, types[1].offset);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].id, 1);

    // Level 3: one language directory each, pointing at a data entry.
    let mut data_entries = Vec::new();
    for entry in images.iter().chain(groups.iter()) {
        let (languages, after) = directory(&section, entry.offset);
        assert_eq!(languages.len(), 1, "one language per resource");
        assert!(!languages[0].is_directory, "a data entry is a leaf");
        assert!(languages[0].offset + 16 <= section.len());
        assert_eq!(languages[0].id, 0x0409, "en-US");
        data_entries.push(languages[0].offset);
        let _ = after;
    }
    assert_eq!(
        after_groups + (count + 1) * 24,
        data_entries[0],
        "the language tables sit between the name tables and the data entries"
    );

    // Level 4: the data entries address the image bytes verbatim.
    let mut blobs = Vec::new();
    for at in &data_entries {
        let at = *at;
        assert_eq!(le32(&section, at + 8), 0, "code page is zero");
        assert_eq!(le32(&section, at + 12), 0, "reserved is zero");
        let address = le32(&section, at) as usize;
        let size = le32(&section, at + 4) as usize;
        assert!(
            address.is_multiple_of(8),
            "data is 8-aligned, got {address}"
        );
        assert!(
            address + size <= section.len(),
            "data entry {at} runs past the section"
        );
        blobs.push(&section[address..address + size]);
    }
    for (slice, icon) in blobs.iter().zip(icons.iter()) {
        assert_eq!(*slice, icon.data.as_slice(), "image bytes survive intact");
    }

    // The group directory names the images by id, in the same order.
    let group = blobs[count];
    assert_eq!(le16(group, 0), 0, "reserved");
    assert_eq!(le16(group, 2), 1, "type: icon");
    assert_eq!(le16(group, 4) as usize, count);
    assert_eq!(group.len(), 6 + 14 * count);
    for (index, icon) in icons.iter().enumerate() {
        let at = 6 + index * 14;
        assert_eq!(group[at], icon.width);
        assert_eq!(group[at + 1], icon.height);
        assert_eq!(le16(group, at + 4), icon.planes);
        assert_eq!(le16(group, at + 6), icon.bit_count);
        assert_eq!(le32(group, at + 8) as usize, icon.data.len());
        assert_eq!(le16(group, at + 12) as usize, index + 1);
    }

    // Nothing overlaps and nothing is left dangling.
    let mut spans: Vec<(usize, usize)> = data_entries
        .iter()
        .map(|at| {
            let address = le32(&section, *at) as usize;
            let size = le32(&section, *at + 4) as usize;
            (address, address + size)
        })
        .collect();
    spans.sort_unstable();
    for pair in spans.windows(2) {
        assert!(
            pair[0].1 <= pair[1].0,
            "data blobs overlap: {pair:?} of {spans:?}"
        );
    }
    assert_eq!(blobs.len(), count + 1);
}

#[test]
fn the_coff_object_is_well_formed() {
    let icons = read_ico(ICON).expect("the committed icon parses");
    for arch in [Arch::Amd64, Arch::I386, Arch::Arm64] {
        let object = coff_object(&icons, arch);
        let expected_machine = match arch {
            Arch::Amd64 => 0x8664,
            Arch::I386 => 0x014c,
            Arch::Arm64 => 0xaa64,
        };
        assert_eq!(le16(&object, 0), expected_machine);
        assert_eq!(le16(&object, 2), 1, "one section");
        assert_eq!(le32(&object, 4), 0, "timestamped for reproducibility");

        let symbols = le32(&object, 8) as usize;
        let nsymbols = le32(&object, 12) as usize;
        assert_eq!(nsymbols, 1, "the section symbol");
        assert_eq!(le16(&object, 16), 0, "no optional header");

        // The section header: name, sizes, and where the raw data lives.
        assert_eq!(&object[20..25], b".rsrc", "section name");
        let size_of_raw_data = le32(&object, 20 + 16) as usize;
        let pointer_to_raw_data = le32(&object, 20 + 20) as usize;
        let pointer_to_relocations = le32(&object, 20 + 24) as usize;
        let nrelocations = le16(&object, 20 + 32) as usize;
        assert_eq!(pointer_to_raw_data, 60, "raw data follows the headers");
        assert_eq!(nrelocations, icons.len() + 1, "one per data entry");
        assert_eq!(
            le32(&object, 20 + 36),
            0x4000_0040,
            "initialized data, readable"
        );

        // The section content is byte-identical to the standalone section.
        let section = rsrc_section(&icons);
        assert_eq!(size_of_raw_data, section.len());
        assert_eq!(
            &object[pointer_to_raw_data..pointer_to_raw_data + size_of_raw_data],
            section.as_slice()
        );
        assert_eq!(
            pointer_to_relocations,
            pointer_to_raw_data + size_of_raw_data,
            "relocations follow the data"
        );

        // Every relocation names a data entry, as an ADDR32NB fixup.
        let expected_reloc = match arch {
            Arch::Amd64 => 0x0003,
            Arch::I386 => 0x0007,
            Arch::Arm64 => 0x0002,
        };
        let mut relocated = Vec::new();
        for index in 0..nrelocations {
            let at = pointer_to_relocations + index * 10;
            relocated.push(le32(&object, at) as usize);
            assert_eq!(le32(&object, at + 4), 0, "symbol 0");
            assert_eq!(le16(&object, at + 8), expected_reloc);
        }
        relocated.sort_unstable();
        let mut entries = Vec::new();
        let (types, _) = directory(&section, 0);
        let (images, _) = directory(&section, types[0].offset);
        let (groups, _) = directory(&section, types[1].offset);
        for entry in images.iter().chain(groups.iter()) {
            let (languages, _) = directory(&section, entry.offset);
            entries.push(languages[0].offset);
        }
        entries.sort_unstable();
        assert_eq!(relocated, entries, "relocations cover every data entry");

        // The symbol table and the (empty) string table.
        assert_eq!(pointer_to_relocations + nrelocations * 10, symbols);
        assert_eq!(&object[symbols..symbols + 5], b".rsrc");
        assert_eq!(le16(&object, symbols + 12), 1, "section number");
        assert_eq!(object[symbols + 16], 3, "IMAGE_SYM_CLASS_STATIC");
        assert_eq!(le32(&object, symbols + 18), 4, "empty string table");
        assert_eq!(object.len(), symbols + 18 + 4);
    }
}
