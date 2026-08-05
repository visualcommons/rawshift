//! Minimal ISOBMFF (HEIF/AVIF) box helpers and item-level metadata surgery.
//!
//! Shared by the ICC (`colr` box splicing) and EXIF (`Exif` item
//! insertion/extraction) embedding paths. This container-side surgery is
//! transitional: it moves behind the gamut codec boundaries when the per-format
//! codec migrations land (the codecs then exchange metadata as codec-side
//! `MetadataBlock`s instead of splicing boxes here).
//!
//! Scope: exactly what the AVIF files rawshift produces/reads need. Extent
//! offsets are treated as absolute file offsets (`construction_method` 0);
//! `idat`-relative storage is not supported.

// ── Byte-level helpers ────────────────────────────────────────────────────────

pub(crate) fn read_u32_be(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
}

pub(crate) fn write_u32_be(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

pub(crate) fn read_uint_be(data: &[u8], offset: usize, size: usize) -> u64 {
    match size {
        0 => 0,
        1 => data[offset] as u64,
        2 => u16::from_be_bytes(data[offset..offset + 2].try_into().unwrap()) as u64,
        4 => u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap()) as u64,
        8 => u64::from_be_bytes(data[offset..offset + 8].try_into().unwrap()),
        _ => 0,
    }
}

pub(crate) fn write_uint_be(data: &mut [u8], offset: usize, size: usize, value: u64) {
    match size {
        0 => {}
        1 => data[offset] = value as u8,
        2 => data[offset..offset + 2].copy_from_slice(&(value as u16).to_be_bytes()),
        4 => data[offset..offset + 4].copy_from_slice(&(value as u32).to_be_bytes()),
        8 => data[offset..offset + 8].copy_from_slice(&value.to_be_bytes()),
        _ => {}
    }
}

/// Find the first box of `box_type` in byte range `[start, end)`.
pub(crate) fn find_box(data: &[u8], start: usize, end: usize, box_type: &[u8; 4]) -> Option<usize> {
    let mut pos = start;
    while pos + 8 <= end.min(data.len()) {
        let size = read_u32_be(data, pos) as usize;
        if size < 8 || pos + size > data.len() {
            break;
        }
        if &data[pos + 4..pos + 8] == box_type {
            return Some(pos);
        }
        pos += size;
    }
    None
}

/// Patch iloc extent offsets by adding `delta` (mdat shifted by this amount).
pub(crate) fn patch_iloc_extents(
    data: &mut [u8],
    iloc_start: usize,
    delta: isize,
) -> Result<(), String> {
    if iloc_start + 16 > data.len() {
        return Err("iloc box too small".into());
    }
    // FullBox: size(4)+type(4)+version(1)+flags(3); version at +8
    let version = data[iloc_start + 8];
    // Nibble fields at +12 and +13
    let offset_size = ((data[iloc_start + 12] >> 4) & 0xF) as usize;
    let length_size = (data[iloc_start + 12] & 0xF) as usize;
    let base_offset_size = ((data[iloc_start + 13] >> 4) & 0xF) as usize;
    let index_size = if version >= 1 {
        (data[iloc_start + 13] & 0xF) as usize
    } else {
        0
    };
    let (item_count, mut pos) = if version < 2 {
        let count = u16::from_be_bytes([data[iloc_start + 14], data[iloc_start + 15]]) as usize;
        (count, iloc_start + 16)
    } else {
        let count = read_u32_be(data, iloc_start + 14) as usize;
        (count, iloc_start + 18)
    };

    for _ in 0..item_count {
        // item_id
        pos += if version < 2 { 2 } else { 4 };
        // construction_method (v1/2)
        if version >= 1 {
            pos += 2;
        }
        // data_reference_index
        pos += 2;
        // base_data_offset (patch if stored and non-zero)
        if base_offset_size > 0 {
            if pos + base_offset_size > data.len() {
                return Err("iloc base_data_offset OOB".into());
            }
            let v = read_uint_be(data, pos, base_offset_size);
            if v > 0 {
                write_uint_be(
                    data,
                    pos,
                    base_offset_size,
                    (v as i64 + delta as i64) as u64,
                );
            }
        }
        pos += base_offset_size;
        // extent_count
        if pos + 2 > data.len() {
            return Err("iloc extent_count OOB".into());
        }
        let extent_count = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;
        for _ in 0..extent_count {
            if version >= 1 {
                pos += index_size;
            }
            // extent_offset — patch
            if offset_size > 0 {
                if pos + offset_size > data.len() {
                    return Err("iloc extent_offset OOB".into());
                }
                let v = read_uint_be(data, pos, offset_size);
                write_uint_be(data, pos, offset_size, (v as i64 + delta as i64) as u64);
            }
            pos += offset_size;
            pos += length_size;
        }
    }
    Ok(())
}

// ── Item-level surgery (used by the EXIF paths) ───────────────────────────────

/// The decoded header of an `iloc` box.
#[cfg(feature = "exif")]
struct IlocInfo {
    size: usize,
    version: u8,
    offset_size: usize,
    length_size: usize,
    base_offset_size: usize,
    index_size: usize,
    item_count: usize,
    entries_start: usize,
}

/// One decoded `iloc` entry.
#[cfg(feature = "exif")]
struct IlocEntry {
    item_id: u32,
    // `construction_method` and `extents` are consumed only by the test-only
    // `extract_item` oracle (production reads moved to gamut-avif in #33);
    // `insert_item` reads `item_id` alone.
    #[cfg_attr(not(test), allow(dead_code))]
    construction_method: u16,
    /// `(absolute offset, length)` pairs (base offset already applied).
    #[cfg_attr(not(test), allow(dead_code))]
    extents: Vec<(u64, u64)>,
}

#[cfg(feature = "exif")]
fn parse_iloc_header(data: &[u8], iloc_start: usize) -> Result<IlocInfo, String> {
    if iloc_start + 16 > data.len() {
        return Err("iloc box too small".into());
    }
    let size = read_u32_be(data, iloc_start) as usize;
    let version = data[iloc_start + 8];
    let offset_size = ((data[iloc_start + 12] >> 4) & 0xF) as usize;
    let length_size = (data[iloc_start + 12] & 0xF) as usize;
    let base_offset_size = ((data[iloc_start + 13] >> 4) & 0xF) as usize;
    let index_size = if version >= 1 {
        (data[iloc_start + 13] & 0xF) as usize
    } else {
        0
    };
    let (item_count, entries_start) = if version < 2 {
        let count = u16::from_be_bytes([data[iloc_start + 14], data[iloc_start + 15]]) as usize;
        (count, iloc_start + 16)
    } else {
        if iloc_start + 18 > data.len() {
            return Err("iloc box too small".into());
        }
        (read_u32_be(data, iloc_start + 14) as usize, iloc_start + 18)
    };
    Ok(IlocInfo {
        size,
        version,
        offset_size,
        length_size,
        base_offset_size,
        index_size,
        item_count,
        entries_start,
    })
}

/// Decode every `iloc` entry, or `None` if the box is malformed/truncated.
#[cfg(feature = "exif")]
fn iloc_entries(data: &[u8], info: &IlocInfo) -> Option<Vec<IlocEntry>> {
    let mut entries = Vec::with_capacity(info.item_count);
    let mut pos = info.entries_start;
    for _ in 0..info.item_count {
        let item_id = if info.version < 2 {
            let id = u16::from_be_bytes(data.get(pos..pos + 2)?.try_into().ok()?);
            pos += 2;
            u32::from(id)
        } else {
            let id = u32::from_be_bytes(data.get(pos..pos + 4)?.try_into().ok()?);
            pos += 4;
            id
        };
        let mut construction_method = 0u16;
        if info.version >= 1 {
            construction_method =
                u16::from_be_bytes(data.get(pos..pos + 2)?.try_into().ok()?) & 0x0F;
            pos += 2;
        }
        pos += 2; // data_reference_index
        if pos + info.base_offset_size > data.len() {
            return None;
        }
        let base = read_uint_be(data, pos, info.base_offset_size);
        pos += info.base_offset_size;
        let extent_count = u16::from_be_bytes(data.get(pos..pos + 2)?.try_into().ok()?) as usize;
        pos += 2;
        let mut extents = Vec::with_capacity(extent_count);
        for _ in 0..extent_count {
            if info.version >= 1 {
                pos += info.index_size;
            }
            if pos + info.offset_size + info.length_size > data.len() {
                return None;
            }
            let offset = read_uint_be(data, pos, info.offset_size);
            pos += info.offset_size;
            let length = read_uint_be(data, pos, info.length_size);
            pos += info.length_size;
            extents.push((base + offset, length));
        }
        entries.push(IlocEntry {
            item_id,
            construction_method,
            extents,
        });
    }
    Some(entries)
}

/// The `(item_id, item_type)` of an `infe` box at `pos`, or `None` for the
/// pre-v2 layouts (which HEIF/AVIF item-info boxes do not use).
#[cfg(feature = "exif")]
fn parse_infe(data: &[u8], pos: usize, size: usize) -> Option<(u32, [u8; 4])> {
    let version = *data.get(pos + 8)?;
    let (item_id, type_at) = match version {
        2 => {
            let id = u16::from_be_bytes(data.get(pos + 12..pos + 14)?.try_into().ok()?);
            (u32::from(id), pos + 16)
        }
        3 => {
            let id = u32::from_be_bytes(data.get(pos + 12..pos + 16)?.try_into().ok()?);
            (id, pos + 18)
        }
        _ => return None,
    };
    if type_at + 4 > pos + size {
        return None;
    }
    let item_type: [u8; 4] = data.get(type_at..type_at + 4)?.try_into().ok()?;
    Some((item_id, item_type))
}

/// Walk the `infe` boxes of an `iinf` box, yielding each `(pos, size)`.
#[cfg(feature = "exif")]
fn infe_boxes(data: &[u8], iinf_start: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
    let iinf_size = read_u32_be(data, iinf_start) as usize;
    let iinf_end = (iinf_start + iinf_size).min(data.len());
    let version = data.get(iinf_start + 8).copied().unwrap_or(0);
    let mut pos = if version == 0 {
        iinf_start + 14
    } else {
        iinf_start + 16
    };
    std::iter::from_fn(move || {
        while pos + 8 <= iinf_end {
            let size = read_u32_be(data, pos) as usize;
            if size < 8 || pos + size > iinf_end {
                return None;
            }
            let current = pos;
            pos += size;
            if &data[current + 4..current + 8] == b"infe" {
                return Some((current, size));
            }
        }
        None
    })
}

/// Extract the payload bytes of the first item of `item_type`, or `None` if
/// the container/item is absent or malformed.
///
/// Production AVIF reads go through gamut-avif's item surface
/// (`formats::avif`) since #33; this reader is kept as the independent oracle
/// the [`insert_item`] tests verify the write path against.
#[cfg(all(test, feature = "exif"))]
pub(crate) fn extract_item(data: &[u8], item_type: [u8; 4]) -> Option<Vec<u8>> {
    let meta_start = find_box(data, 0, data.len(), b"meta")?;
    let meta_end = meta_start + read_u32_be(data, meta_start) as usize;
    let content = meta_start + 12;

    let iinf_start = find_box(data, content, meta_end, b"iinf")?;
    let item_id =
        infe_boxes(data, iinf_start).find_map(|(pos, size)| match parse_infe(data, pos, size) {
            Some((id, ty)) if ty == item_type => Some(id),
            _ => None,
        })?;

    let iloc_start = find_box(data, content, meta_end, b"iloc")?;
    let info = parse_iloc_header(data, iloc_start).ok()?;
    let entry = iloc_entries(data, &info)?
        .into_iter()
        .find(|e| e.item_id == item_id)?;
    if entry.construction_method != 0 {
        return None; // only absolute file offsets are supported
    }

    let mut out = Vec::new();
    for (offset, length) in entry.extents {
        let start = usize::try_from(offset).ok()?;
        let end = start.checked_add(usize::try_from(length).ok()?)?;
        out.extend_from_slice(data.get(start..end)?);
    }
    (!out.is_empty()).then_some(out)
}

/// Insert `payload` as a new item of `item_type`, described by (`cdsc`)
/// referencing the primary item.
///
/// The payload is appended as a trailing `mdat` box; the `iinf`/`iloc`/`iref`
/// boxes gain the matching entries and every existing extent offset is shifted
/// by the growth of the `meta` box.
#[cfg(feature = "exif")]
pub(crate) fn insert_item(
    mut data: Vec<u8>,
    item_type: [u8; 4],
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let data_len = data.len();
    let meta_start = find_box(&data, 0, data_len, b"meta").ok_or("no meta box")?;
    let meta_size = read_u32_be(&data, meta_start) as usize;
    let meta_end = meta_start + meta_size;
    let content = meta_start + 12;

    // Primary item (the `cdsc` reference target).
    let pitm_start = find_box(&data, content, meta_end, b"pitm").ok_or("no pitm box")?;
    let pitm_version = *data.get(pitm_start + 8).ok_or("pitm box too small")?;
    let primary_id = if pitm_version == 0 {
        let raw: [u8; 2] = data
            .get(pitm_start + 12..pitm_start + 14)
            .ok_or("pitm box too small")?
            .try_into()
            .unwrap();
        u32::from(u16::from_be_bytes(raw))
    } else {
        let raw: [u8; 4] = data
            .get(pitm_start + 12..pitm_start + 16)
            .ok_or("pitm box too small")?
            .try_into()
            .unwrap();
        u32::from_be_bytes(raw)
    };

    // iinf: allocate a fresh item id above every id in use.
    let iinf_start = find_box(&data, content, meta_end, b"iinf").ok_or("no iinf box")?;
    let iinf_size = read_u32_be(&data, iinf_start) as usize;
    let iinf_version = *data.get(iinf_start + 8).ok_or("iinf box too small")?;

    // iloc: the new entry must be expressible with the file's field widths.
    let iloc_start = find_box(&data, content, meta_end, b"iloc").ok_or("no iloc box")?;
    let info = parse_iloc_header(&data, iloc_start)?;
    if info.offset_size != 4 && info.offset_size != 8 {
        return Err(format!("unsupported iloc offset_size {}", info.offset_size));
    }
    if !matches!(info.length_size, 1 | 2 | 4 | 8) {
        return Err(format!("unsupported iloc length_size {}", info.length_size));
    }
    if info.length_size < 8 && (payload.len() as u64) >= 1u64 << (8 * info.length_size) {
        return Err("payload too large for the iloc length field".into());
    }
    let entries = iloc_entries(&data, &info).ok_or("malformed iloc box")?;

    let infe_max = infe_boxes(&data, iinf_start)
        .filter_map(|(pos, size)| parse_infe(&data, pos, size).map(|(id, _)| id))
        .max()
        .unwrap_or(0);
    let iloc_max = entries.iter().map(|e| e.item_id).max().unwrap_or(0);
    let new_id = infe_max
        .max(iloc_max)
        .max(primary_id)
        .checked_add(1)
        .ok_or("item id overflow")?;
    if info.version < 2 && new_id > u32::from(u16::MAX) {
        return Err("item id exceeds the iloc v0/v1 range".into());
    }

    // iref: append to an existing box or create a fresh one at the end of meta.
    let iref_existing = find_box(&data, content, meta_end, b"iref");
    let (iref_insert_pos, iref_bytes) = match iref_existing {
        Some(iref_start) => {
            let iref_size = read_u32_be(&data, iref_start) as usize;
            let iref_version = *data.get(iref_start + 8).ok_or("iref box too small")?;
            if iref_version == 0
                && (new_id > u32::from(u16::MAX) || primary_id > u32::from(u16::MAX))
            {
                return Err("item id exceeds the iref v0 range".into());
            }
            (
                iref_start + iref_size,
                build_reference(iref_version, new_id, primary_id),
            )
        }
        None => {
            let wide = new_id > u32::from(u16::MAX) || primary_id > u32::from(u16::MAX);
            let version: u8 = if wide { 1 } else { 0 };
            let reference = build_reference(version, new_id, primary_id);
            let mut boxed = Vec::with_capacity(12 + reference.len());
            boxed.extend_from_slice(&((12 + reference.len()) as u32).to_be_bytes());
            boxed.extend_from_slice(b"iref");
            boxed.push(version);
            boxed.extend_from_slice(&[0, 0, 0]); // flags
            boxed.extend_from_slice(&reference);
            (meta_end, boxed)
        }
    };

    let infe_box = build_infe(new_id, item_type);
    let iloc_entry_len = iloc_entry_len(&info);
    let total_delta = infe_box.len() + iloc_entry_len + iref_bytes.len();

    // 1. Shift every existing extent offset by the meta growth (mdat moves).
    patch_iloc_extents(&mut data, iloc_start, total_delta as isize)?;

    // 2. The payload lands in a fresh mdat box appended at the end of the
    //    grown file; 8 bytes skip that box's header.
    let payload_offset = (data_len + total_delta + 8) as u64;
    if info.offset_size < 8 && payload_offset >= 1u64 << (8 * info.offset_size) {
        return Err("extent offset exceeds the iloc offset field width".into());
    }
    let iloc_entry = build_iloc_entry(&info, new_id, payload_offset, payload.len() as u64);
    debug_assert_eq!(iloc_entry.len(), iloc_entry_len);

    // 3. Patch the headers (fixed-width edits at their original positions).
    write_u32_be(&mut data, meta_start, (meta_size + total_delta) as u32);
    write_u32_be(&mut data, iinf_start, (iinf_size + infe_box.len()) as u32);
    bump_count(&mut data, iinf_start + 12, iinf_version == 0)?;
    write_u32_be(&mut data, iloc_start, (info.size + iloc_entry_len) as u32);
    bump_count(&mut data, iloc_start + 14, info.version < 2)?;
    if let Some(iref_start) = iref_existing {
        let iref_size = read_u32_be(&data, iref_start) as usize;
        write_u32_be(&mut data, iref_start, (iref_size + iref_bytes.len()) as u32);
    }

    // 4. Splice the insertions, highest position first so the lower insertion
    //    points stay valid. A freshly created iref box can share its insertion
    //    position with the infe/iloc appends (when iinf/iloc is the last child
    //    of meta): the iref bytes belong *after* the in-box appends in the
    //    final layout, so on a positional tie they must be spliced first
    //    (later splices at the same position land earlier in the file).
    let mut inserts: Vec<(usize, u8, Vec<u8>)> = vec![
        (iinf_start + iinf_size, 0, infe_box),
        (iloc_start + info.size, 0, iloc_entry),
        (iref_insert_pos, 1, iref_bytes),
    ];
    inserts.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    for (pos, _, bytes) in inserts {
        data.splice(pos..pos, bytes);
    }

    // 5. Append the payload as a trailing mdat box.
    data.reserve(8 + payload.len());
    data.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
    data.extend_from_slice(b"mdat");
    data.extend_from_slice(payload);

    Ok(data)
}

/// Increment a big-endian item count (`u16` when `narrow`, `u32` otherwise).
#[cfg(feature = "exif")]
fn bump_count(data: &mut [u8], pos: usize, narrow: bool) -> Result<(), String> {
    if narrow {
        let raw: [u8; 2] = data
            .get(pos..pos + 2)
            .ok_or("item count out of bounds")?
            .try_into()
            .unwrap();
        let count = u16::from_be_bytes(raw)
            .checked_add(1)
            .ok_or("item count overflow")?;
        data[pos..pos + 2].copy_from_slice(&count.to_be_bytes());
    } else {
        let raw: [u8; 4] = data
            .get(pos..pos + 4)
            .ok_or("item count out of bounds")?
            .try_into()
            .unwrap();
        let count = u32::from_be_bytes(raw)
            .checked_add(1)
            .ok_or("item count overflow")?;
        data[pos..pos + 4].copy_from_slice(&count.to_be_bytes());
    }
    Ok(())
}

/// Build an `infe` box (v2 for 16-bit ids, v3 otherwise) with an empty name.
#[cfg(feature = "exif")]
fn build_infe(item_id: u32, item_type: [u8; 4]) -> Vec<u8> {
    let version: u8 = if item_id <= u32::from(u16::MAX) { 2 } else { 3 };
    let id_len = if version == 2 { 2 } else { 4 };
    let size = 8 + 4 + id_len + 2 + 4 + 1;
    let mut b = Vec::with_capacity(size);
    b.extend_from_slice(&(size as u32).to_be_bytes());
    b.extend_from_slice(b"infe");
    b.push(version);
    b.extend_from_slice(&[0, 0, 0]); // flags
    if version == 2 {
        b.extend_from_slice(&(item_id as u16).to_be_bytes());
    } else {
        b.extend_from_slice(&item_id.to_be_bytes());
    }
    b.extend_from_slice(&0u16.to_be_bytes()); // item_protection_index
    b.extend_from_slice(&item_type);
    b.push(0); // empty item_name
    b
}

/// Build a `cdsc` SingleItemTypeReferenceBox (`from_id` describes `to_id`).
#[cfg(feature = "exif")]
fn build_reference(iref_version: u8, from_id: u32, to_id: u32) -> Vec<u8> {
    let id_len = if iref_version == 0 { 2 } else { 4 };
    let size = 8 + id_len + 2 + id_len;
    let mut b = Vec::with_capacity(size);
    b.extend_from_slice(&(size as u32).to_be_bytes());
    b.extend_from_slice(b"cdsc");
    if iref_version == 0 {
        b.extend_from_slice(&(from_id as u16).to_be_bytes());
        b.extend_from_slice(&1u16.to_be_bytes()); // reference_count
        b.extend_from_slice(&(to_id as u16).to_be_bytes());
    } else {
        b.extend_from_slice(&from_id.to_be_bytes());
        b.extend_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&to_id.to_be_bytes());
    }
    b
}

/// The on-disk length of a single-extent `iloc` entry under `info`'s widths.
#[cfg(feature = "exif")]
fn iloc_entry_len(info: &IlocInfo) -> usize {
    let id_len = if info.version < 2 { 2 } else { 4 };
    let method_len = if info.version >= 1 { 2 } else { 0 };
    let index_len = if info.version >= 1 {
        info.index_size
    } else {
        0
    };
    id_len
        + method_len
        + 2
        + info.base_offset_size
        + 2
        + index_len
        + info.offset_size
        + info.length_size
}

/// Build a single-extent `iloc` entry (absolute offset, zero base offset).
#[cfg(feature = "exif")]
fn build_iloc_entry(info: &IlocInfo, item_id: u32, offset: u64, length: u64) -> Vec<u8> {
    fn push_uint_be(out: &mut Vec<u8>, size: usize, value: u64) {
        match size {
            0 => {}
            1 => out.push(value as u8),
            2 => out.extend_from_slice(&(value as u16).to_be_bytes()),
            4 => out.extend_from_slice(&(value as u32).to_be_bytes()),
            8 => out.extend_from_slice(&value.to_be_bytes()),
            _ => {}
        }
    }

    let mut b = Vec::with_capacity(iloc_entry_len(info));
    if info.version < 2 {
        b.extend_from_slice(&(item_id as u16).to_be_bytes());
    } else {
        b.extend_from_slice(&item_id.to_be_bytes());
    }
    if info.version >= 1 {
        b.extend_from_slice(&0u16.to_be_bytes()); // construction_method: file offset
    }
    b.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
    push_uint_be(&mut b, info.base_offset_size, 0);
    b.extend_from_slice(&1u16.to_be_bytes()); // extent_count
    if info.version >= 1 {
        push_uint_be(&mut b, info.index_size, 0);
    }
    push_uint_be(&mut b, info.offset_size, offset);
    push_uint_be(&mut b, info.length_size, length);
    b
}

#[cfg(all(test, feature = "exif"))]
mod tests {
    use super::*;

    /// Build a minimal AVIF-shaped container: ftyp, meta{hdlr, pitm(id=1),
    /// iloc(v0, one entry for the primary), iinf(v0, one `av01` infe)}, mdat.
    fn synthetic_avif(image_payload: &[u8]) -> Vec<u8> {
        fn boxed(box_type: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut b = Vec::with_capacity(8 + body.len());
            b.extend_from_slice(&((8 + body.len()) as u32).to_be_bytes());
            b.extend_from_slice(box_type);
            b.extend_from_slice(body);
            b
        }
        fn full_boxed(box_type: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
            let mut inner = vec![version, 0, 0, 0];
            inner.extend_from_slice(body);
            boxed(box_type, &inner)
        }

        let ftyp = boxed(b"ftyp", b"avif\x00\x00\x00\x00avifmif1");
        let hdlr = full_boxed(
            b"hdlr",
            0,
            b"\x00\x00\x00\x00pict\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
        );
        let pitm = full_boxed(b"pitm", 0, &1u16.to_be_bytes());
        let infe = {
            let mut body = Vec::new();
            body.extend_from_slice(&1u16.to_be_bytes()); // item_id
            body.extend_from_slice(&0u16.to_be_bytes()); // protection
            body.extend_from_slice(b"av01");
            body.push(0); // name
            full_boxed(b"infe", 2, &body)
        };
        let iinf = {
            let mut body = Vec::new();
            body.extend_from_slice(&1u16.to_be_bytes()); // entry_count
            body.extend_from_slice(&infe);
            full_boxed(b"iinf", 0, &body)
        };
        // iloc v0: offset_size=4, length_size=4, base_offset_size=0.
        // The extent offset is patched below once the layout is known.
        let iloc = {
            let mut body = Vec::new();
            body.push(0x44); // offset_size | length_size
            body.push(0x00); // base_offset_size | reserved
            body.extend_from_slice(&1u16.to_be_bytes()); // item_count
            body.extend_from_slice(&1u16.to_be_bytes()); // item_id
            body.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
            body.extend_from_slice(&1u16.to_be_bytes()); // extent_count
            body.extend_from_slice(&0u32.to_be_bytes()); // extent_offset (patched)
            body.extend_from_slice(&(image_payload.len() as u32).to_be_bytes());
            full_boxed(b"iloc", 0, &body)
        };
        let meta = {
            let mut body = Vec::new();
            body.extend_from_slice(&hdlr);
            body.extend_from_slice(&pitm);
            body.extend_from_slice(&iloc);
            body.extend_from_slice(&iinf);
            full_boxed(b"meta", 0, &body)
        };
        let mdat = boxed(b"mdat", image_payload);

        let mut file = Vec::new();
        file.extend_from_slice(&ftyp);
        file.extend_from_slice(&meta);
        let image_offset = (file.len() + 8) as u32;
        file.extend_from_slice(&mdat);

        // Patch the primary item's extent offset now that mdat's position is
        // known: it is the last 8..4 bytes of the iloc body inside meta.
        let meta_start = find_box(&file, 0, file.len(), b"meta").unwrap();
        let meta_end = meta_start + read_u32_be(&file, meta_start) as usize;
        let iloc_start = find_box(&file, meta_start + 12, meta_end, b"iloc").unwrap();
        let iloc_size = read_u32_be(&file, iloc_start) as usize;
        write_u32_be(&mut file, iloc_start + iloc_size - 8, image_offset);
        file
    }

    #[test]
    fn insert_then_extract_round_trips() {
        let image = b"primary image payload".to_vec();
        let avif = synthetic_avif(&image);
        let payload = b"\x00\x00\x00\x00II*\x00fake-tiff".to_vec();

        let out = insert_item(avif, *b"Exif", &payload).expect("insert");
        let extracted = extract_item(&out, *b"Exif").expect("extract");
        assert_eq!(extracted, payload);

        // The primary item's extent must still point at the image payload.
        let primary = extract_item(&out, *b"av01").expect("primary still readable");
        assert_eq!(primary, image);

        // An iref box with a cdsc reference must have been created.
        let meta_start = find_box(&out, 0, out.len(), b"meta").unwrap();
        let meta_end = meta_start + read_u32_be(&out, meta_start) as usize;
        let iref = find_box(&out, meta_start + 12, meta_end, b"iref");
        assert!(iref.is_some(), "cdsc reference box must exist");
    }

    #[test]
    fn insert_twice_allocates_distinct_items() {
        let avif = synthetic_avif(b"img");
        let one = insert_item(avif, *b"Exif", b"payload-1").expect("first insert");
        let two = insert_item(one, *b"xml ", b"payload-2").expect("second insert");
        assert_eq!(extract_item(&two, *b"Exif").unwrap(), b"payload-1");
        assert_eq!(extract_item(&two, *b"xml ").unwrap(), b"payload-2");
        assert_eq!(extract_item(&two, *b"av01").unwrap(), b"img");
    }

    #[test]
    fn extract_missing_item_is_none() {
        let avif = synthetic_avif(b"img");
        assert!(extract_item(&avif, *b"Exif").is_none());
        assert!(extract_item(b"not a container", *b"Exif").is_none());
    }

    #[test]
    fn insert_into_garbage_errors() {
        assert!(insert_item(b"garbage".to_vec(), *b"Exif", b"p").is_err());
    }
}
