//! A focused ISOBMFF header walk for the facts the demux backend does not
//! surface.
//!
//! This is **not** a demuxer and must not grow into one. It reads box headers
//! and four small boxes — `ftyp`, `mvhd`, `tkhd`, `mdhd` — because
//! `symphonia-format-isomp4` exposes neither the track display matrix (and so
//! no rotation, without which most phone video presents sideways) nor the
//! `ftyp` brands. Everything else about the file comes from symphonia.
//!
//! Every read is bounds-checked and every container descent is depth- and
//! size-limited: this parses attacker-supplied bytes.

use std::io::{self, Read, Seek, SeekFrom};

use rawshift_video_core::{ContainerId, Rotation, VideoResult};

/// How deep to descend into nested boxes.
///
/// The tree this walks is `moov > trak > mdia > mdhd`, so four levels are
/// enough; the limit stops a crafted file from recursing without bound.
const MAX_DEPTH: u32 = 8;

/// The largest box header this walker will honour, as a sanity bound against
/// a crafted 64-bit size.
const MAX_BOX_SIZE: u64 = 1 << 40;

/// What the header walk recovered.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BoxSurvey {
    /// `ftyp` major brand, then compatible brands.
    pub brands: Vec<String>,
    /// `mvhd` timescale, in ticks per second.
    pub timescale: Option<u32>,
    /// `mvhd` duration, in timescale ticks.
    pub duration: Option<u64>,
    /// `mvhd` creation time, in seconds since 1904-01-01 UTC.
    pub creation_time: Option<u64>,
    /// `mvhd` modification time, in the same epoch.
    pub modification_time: Option<u64>,
    /// Per-track facts, keyed by `tkhd` track id.
    pub tracks: Vec<TrackSurvey>,
}

/// Per-track facts from `tkhd`.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackSurvey {
    /// The `tkhd` track identifier.
    pub id: u32,
    /// Display rotation recovered from the `tkhd` matrix.
    pub rotation: Rotation,
}

impl BoxSurvey {
    /// The rotation for a track id, or [`Rotation::None`] if not seen.
    #[must_use]
    pub fn rotation(&self, track_id: u32) -> Rotation {
        self.tracks
            .iter()
            .find(|t| t.id == track_id)
            .map_or(Rotation::None, |t| t.rotation)
    }

    /// The container flavour implied by the `ftyp` brands.
    ///
    /// The QuickTime brand `qt  ` means [`ContainerId::Mov`]; anything else
    /// with a `ftyp` is treated as MP4. Files with no `ftyp` at all are
    /// old-style QuickTime.
    #[must_use]
    pub fn container(&self) -> ContainerId {
        if self.brands.is_empty() {
            return ContainerId::Mov;
        }
        if self.brands.iter().any(|b| b == "qt  ") {
            ContainerId::Mov
        } else {
            ContainerId::Mp4
        }
    }
}

/// Walk the box tree of `reader`, recovering [`BoxSurvey`].
///
/// Leaves the reader's position undefined; the caller rewinds.
///
/// # Errors
///
/// Propagates I/O failures. A structurally odd file is *not* an error: the
/// walk stops and returns what it has, because these facts are supplementary
/// and a file whose `tkhd` cannot be read should still demux.
pub fn survey<R: Read + Seek>(reader: &mut R) -> VideoResult<BoxSurvey> {
    let end = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;

    let mut out = BoxSurvey::default();
    walk(reader, 0, end, 0, &mut out)?;
    Ok(out)
}

/// One box header: its payload range and type.
struct BoxHeader {
    kind: [u8; 4],
    payload_start: u64,
    payload_end: u64,
}

/// Read a box header at `offset`, or `None` at a clean end or on anything
/// malformed.
fn read_header<R: Read + Seek>(
    reader: &mut R,
    offset: u64,
    limit: u64,
) -> io::Result<Option<BoxHeader>> {
    if offset.saturating_add(8) > limit {
        return Ok(None);
    }
    reader.seek(SeekFrom::Start(offset))?;

    let mut header = [0u8; 8];
    if reader.read_exact(&mut header).is_err() {
        return Ok(None);
    }
    let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let kind = [header[4], header[5], header[6], header[7]];

    let (size, payload_start) = match size32 {
        // 1 means the real size is in a following 64-bit field.
        1 => {
            let mut extended = [0u8; 8];
            if reader.read_exact(&mut extended).is_err() {
                return Ok(None);
            }
            (u64::from_be_bytes(extended), offset + 16)
        }
        // 0 means "extends to the end of the enclosing container".
        0 => (limit - offset, offset + 8),
        n => (u64::from(n), offset + 8),
    };

    // A box must at least contain its own header, and must not overrun its
    // parent or exceed the sanity bound.
    let header_len = payload_start - offset;
    if size < header_len || size > MAX_BOX_SIZE {
        return Ok(None);
    }
    let payload_end = offset.saturating_add(size).min(limit);
    if payload_end < payload_start {
        return Ok(None);
    }

    Ok(Some(BoxHeader {
        kind,
        payload_start,
        payload_end,
    }))
}

/// Descend through `[start, limit)`, collecting into `out`.
fn walk<R: Read + Seek>(
    reader: &mut R,
    depth: u32,
    limit: u64,
    start: u64,
    out: &mut BoxSurvey,
) -> VideoResult<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }

    let mut offset = start;
    while offset < limit {
        let Some(header) = read_header(reader, offset, limit)? else {
            return Ok(());
        };
        // A zero-length advance would spin forever on a crafted file.
        if header.payload_end <= offset {
            return Ok(());
        }

        match &header.kind {
            b"ftyp" => read_ftyp(reader, &header, out)?,
            b"mvhd" => read_mvhd(reader, &header, out)?,
            b"tkhd" => read_tkhd(reader, &header, out)?,
            // Containers worth descending into. `moof`/`traf` are skipped:
            // fragments repeat no tkhd, and symphonia owns them.
            b"moov" | b"trak" | b"mdia" => {
                walk(
                    reader,
                    depth + 1,
                    header.payload_end,
                    header.payload_start,
                    out,
                )?;
            }
            _ => {}
        }

        offset = header.payload_end;
    }
    Ok(())
}

/// Read the whole payload of a small box, refusing anything implausible.
fn read_payload<R: Read + Seek>(
    reader: &mut R,
    header: &BoxHeader,
    max: usize,
) -> io::Result<Option<Vec<u8>>> {
    let len = (header.payload_end - header.payload_start) as usize;
    if len > max {
        return Ok(None);
    }
    reader.seek(SeekFrom::Start(header.payload_start))?;
    let mut buf = vec![0u8; len];
    if reader.read_exact(&mut buf).is_err() {
        return Ok(None);
    }
    Ok(Some(buf))
}

fn read_ftyp<R: Read + Seek>(
    reader: &mut R,
    header: &BoxHeader,
    out: &mut BoxSurvey,
) -> VideoResult<()> {
    // major brand + minor version + compatible brands; a few hundred bytes at
    // the very most.
    let Some(payload) = read_payload(reader, header, 1024)? else {
        return Ok(());
    };
    if payload.len() < 4 {
        return Ok(());
    }

    let mut brands = vec![brand_string(&payload[0..4])];
    // Skip the 4-byte minor version, then read 4-byte compatible brands.
    for chunk in payload.get(8..).unwrap_or_default().chunks_exact(4) {
        brands.push(brand_string(chunk));
    }
    out.brands = brands;
    Ok(())
}

/// Render a four-character brand, replacing non-printable bytes so a crafted
/// brand cannot smuggle control characters into a log line or error message.
fn brand_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '?'
            }
        })
        .collect()
}

fn read_mvhd<R: Read + Seek>(
    reader: &mut R,
    header: &BoxHeader,
    out: &mut BoxSurvey,
) -> VideoResult<()> {
    let Some(payload) = read_payload(reader, header, 256)? else {
        return Ok(());
    };
    // version(1) + flags(3), then the version-dependent block.
    let Some(&version) = payload.first() else {
        return Ok(());
    };

    let (creation, modification, timescale, duration) = match version {
        1 => {
            if payload.len() < 4 + 28 {
                return Ok(());
            }
            (
                be64(&payload[4..12]),
                be64(&payload[12..20]),
                be32(&payload[20..24]),
                be64(&payload[24..32]),
            )
        }
        _ => {
            if payload.len() < 4 + 16 {
                return Ok(());
            }
            (
                u64::from(be32(&payload[4..8])),
                u64::from(be32(&payload[8..12])),
                be32(&payload[12..16]),
                u64::from(be32(&payload[16..20])),
            )
        }
    };

    out.creation_time = Some(creation);
    out.modification_time = Some(modification);
    out.timescale = (timescale != 0).then_some(timescale);
    // 0xffff_ffff / u64::MAX is the "unknown duration" sentinel.
    out.duration = (duration != u64::from(u32::MAX) && duration != u64::MAX).then_some(duration);
    Ok(())
}

fn read_tkhd<R: Read + Seek>(
    reader: &mut R,
    header: &BoxHeader,
    out: &mut BoxSurvey,
) -> VideoResult<()> {
    let Some(payload) = read_payload(reader, header, 256)? else {
        return Ok(());
    };
    let Some(&version) = payload.first() else {
        return Ok(());
    };

    // ISO/IEC 14496-12 §8.3.2. The track id follows version+flags and the two
    // times, which are 8 bytes each in version 1 and 4 in version 0; the
    // matrix then follows reserved, duration, reserved[2], layer,
    // alternate_group, volume and one more reserved.
    //
    //   v0: 4 +4+4 =12 (id) … +4+4 +8 +2+2 +2+2 = 40 (matrix)
    //   v1: 4 +8+8 =20 (id) … +4+8 +8 +2+2 +2+2 = 52 (matrix)
    let (id_offset, matrix_offset) = if version == 1 { (20, 52) } else { (12, 40) };

    let Some(id_bytes) = payload.get(id_offset..id_offset + 4) else {
        return Ok(());
    };
    let id = be32(id_bytes);

    let rotation = payload
        .get(matrix_offset..matrix_offset + 36)
        .map_or(Rotation::None, rotation_from_matrix);

    out.tracks.push(TrackSurvey { id, rotation });
    Ok(())
}

/// Recover rotation from the nine 32-bit fixed-point values of a `tkhd`
/// matrix.
///
/// Only `a`, `b`, `c`, `d` — indices 0, 1, 3, 4 — carry rotation. They are
/// 16.16 fixed point (`u`, `v`, `w` are 2.30, and unused here).
fn rotation_from_matrix(matrix: &[u8]) -> Rotation {
    let fixed = |index: usize| -> f64 {
        let at = index * 4;
        f64::from(i32::from_be_bytes([
            matrix[at],
            matrix[at + 1],
            matrix[at + 2],
            matrix[at + 3],
        ])) / 65536.0
    };
    Rotation::from_display_matrix(fixed(0), fixed(1), fixed(3), fixed(4))
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn be64(bytes: &[u8]) -> u64 {
    let mut out = [0u8; 8];
    out.copy_from_slice(&bytes[..8]);
    u64::from_be_bytes(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Build a box: 4-byte size, 4-byte type, payload.
    fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    /// A 16.16 fixed-point matrix from its rotation sub-matrix.
    fn matrix(a: f64, b: f64, c: f64, d: f64) -> Vec<u8> {
        let fx = |v: f64| ((v * 65536.0) as i32).to_be_bytes().to_vec();
        let mut out = Vec::new();
        out.extend(fx(a));
        out.extend(fx(b));
        out.extend(vec![0u8; 4]); // u
        out.extend(fx(c));
        out.extend(fx(d));
        out.extend(vec![0u8; 4]); // v
        out.extend(vec![0u8; 12]); // x, y, w
        out
    }

    fn tkhd_v0(track_id: u32, m: &[u8]) -> Vec<u8> {
        let mut p = vec![0u8; 4]; // version 0 + flags
        p.extend_from_slice(&0u32.to_be_bytes()); // creation
        p.extend_from_slice(&0u32.to_be_bytes()); // modification
        p.extend_from_slice(&track_id.to_be_bytes()); // track id @ 12
        p.extend_from_slice(&0u32.to_be_bytes()); // reserved
        p.extend_from_slice(&0u32.to_be_bytes()); // duration
        p.extend(vec![0u8; 8]); // reserved
        p.extend(vec![0u8; 8]); // layer, alt group, volume, reserved
        assert_eq!(p.len(), 40, "matrix must land at offset 40 in a v0 tkhd");
        p.extend_from_slice(m);
        p.extend(vec![0u8; 8]); // width, height
        p
    }

    fn mvhd_v0(timescale: u32, duration: u32) -> Vec<u8> {
        let mut p = vec![0u8; 4];
        p.extend_from_slice(&1_000u32.to_be_bytes()); // creation
        p.extend_from_slice(&2_000u32.to_be_bytes()); // modification
        p.extend_from_slice(&timescale.to_be_bytes());
        p.extend_from_slice(&duration.to_be_bytes());
        p.extend(vec![0u8; 80]);
        p
    }

    fn file(parts: &[Vec<u8>]) -> Cursor<Vec<u8>> {
        Cursor::new(parts.concat())
    }

    #[test]
    fn brands_come_from_ftyp() {
        let mut ftyp = b"isom".to_vec();
        ftyp.extend_from_slice(&512u32.to_be_bytes());
        ftyp.extend_from_slice(b"isomiso2avc1mp41");
        let mut f = file(&[bx(b"ftyp", &ftyp)]);

        let s = survey(&mut f).expect("survey");
        assert_eq!(s.brands, ["isom", "isom", "iso2", "avc1", "mp41"]);
        assert_eq!(s.container(), ContainerId::Mp4);
    }

    #[test]
    fn the_quicktime_brand_selects_mov() {
        let mut ftyp = b"qt  ".to_vec();
        ftyp.extend_from_slice(&0u32.to_be_bytes());
        ftyp.extend_from_slice(b"qt  ");
        let mut f = file(&[bx(b"ftyp", &ftyp)]);

        assert_eq!(
            survey(&mut f).expect("survey").container(),
            ContainerId::Mov
        );
    }

    #[test]
    fn a_file_with_no_ftyp_is_old_style_quicktime() {
        let mut f = file(&[bx(b"moov", &[])]);
        assert_eq!(
            survey(&mut f).expect("survey").container(),
            ContainerId::Mov
        );
    }

    #[test]
    fn a_brand_with_control_bytes_cannot_smuggle_them_into_a_message() {
        let mut ftyp = vec![0x00, 0x1b, 0x5b, 0x41];
        ftyp.extend_from_slice(&0u32.to_be_bytes());
        let mut f = file(&[bx(b"ftyp", &ftyp)]);

        let s = survey(&mut f).expect("survey");
        // The NUL and the ESC are replaced; the printable bytes are kept, so
        // no control character can reach a log line or error message.
        assert_eq!(s.brands, ["??[A"]);
    }

    #[test]
    fn mvhd_yields_timescale_and_times() {
        let moov = bx(b"mvhd", &mvhd_v0(600, 12_000));
        let mut f = file(&[bx(b"moov", &moov)]);

        let s = survey(&mut f).expect("survey");
        assert_eq!(s.timescale, Some(600));
        assert_eq!(s.duration, Some(12_000));
        assert_eq!(s.creation_time, Some(1_000));
        assert_eq!(s.modification_time, Some(2_000));
    }

    #[test]
    fn an_unknown_duration_sentinel_is_reported_as_absent() {
        let moov = bx(b"mvhd", &mvhd_v0(600, u32::MAX));
        let mut f = file(&[bx(b"moov", &moov)]);
        assert_eq!(survey(&mut f).expect("survey").duration, None);
    }

    #[test]
    fn a_zero_timescale_is_reported_as_absent_rather_than_dividing_by_zero() {
        let moov = bx(b"mvhd", &mvhd_v0(0, 100));
        let mut f = file(&[bx(b"moov", &moov)]);
        assert_eq!(survey(&mut f).expect("survey").timescale, None);
    }

    #[test]
    fn rotation_is_recovered_per_track_from_the_tkhd_matrix() {
        // Track 1 upright, track 2 rotated 90° as a portrait phone capture.
        let trak1 = bx(b"tkhd", &tkhd_v0(1, &matrix(1.0, 0.0, 0.0, 1.0)));
        let trak2 = bx(b"tkhd", &tkhd_v0(2, &matrix(0.0, 1.0, -1.0, 0.0)));
        let moov = [bx(b"trak", &trak1), bx(b"trak", &trak2)].concat();
        let mut f = file(&[bx(b"moov", &moov)]);

        let s = survey(&mut f).expect("survey");
        assert_eq!(s.tracks.len(), 2);
        assert_eq!(s.rotation(1), Rotation::None);
        assert_eq!(s.rotation(2), Rotation::Clockwise90);
        // A track the file does not have reports no rotation, not a panic.
        assert_eq!(s.rotation(99), Rotation::None);
    }

    fn tkhd_v1(track_id: u32, m: &[u8]) -> Vec<u8> {
        let mut p = vec![1u8, 0, 0, 0]; // version 1 + flags
        p.extend(vec![0u8; 8]); // creation (64-bit)
        p.extend(vec![0u8; 8]); // modification (64-bit)
        p.extend_from_slice(&track_id.to_be_bytes()); // track id @ 20
        p.extend_from_slice(&0u32.to_be_bytes()); // reserved
        p.extend(vec![0u8; 8]); // duration (64-bit)
        p.extend(vec![0u8; 8]); // reserved
        p.extend(vec![0u8; 4]); // layer, alternate group
        p.extend(vec![0u8; 4]); // volume, reserved
        assert_eq!(p.len(), 52, "matrix must land at offset 52 in a v1 tkhd");
        p.extend_from_slice(m);
        p.extend(vec![0u8; 8]); // width, height
        p
    }

    #[test]
    fn a_version_1_tkhd_uses_its_own_field_offsets() {
        // Version 1 widens the times to 64 bits, moving both the track id and
        // the matrix. Getting either offset wrong silently yields a wrong
        // track id and no rotation, which is why this is tested separately.
        let trak = bx(b"tkhd", &tkhd_v1(7, &matrix(0.0, -1.0, 1.0, 0.0)));
        let mut f = file(&[bx(b"moov", &bx(b"trak", &trak))]);

        let s = survey(&mut f).expect("survey");
        assert_eq!(s.tracks.len(), 1);
        assert_eq!(s.tracks[0].id, 7, "track id offset");
        assert_eq!(s.rotation(7), Rotation::Clockwise270, "matrix offset");
    }

    #[test]
    fn a_64_bit_box_size_is_honoured() {
        // size==1 means the real size follows as a 64-bit field.
        let payload = mvhd_v0(90_000, 180_000);
        let mut mvhd = 1u32.to_be_bytes().to_vec();
        mvhd.extend_from_slice(b"mvhd");
        mvhd.extend_from_slice(&((payload.len() + 16) as u64).to_be_bytes());
        mvhd.extend_from_slice(&payload);
        let mut f = file(&[bx(b"moov", &mvhd)]);

        assert_eq!(survey(&mut f).expect("survey").timescale, Some(90_000));
    }

    #[test]
    fn a_zero_size_box_terminates_rather_than_spinning() {
        // size==0 means "to the end of the container"; a following box would
        // be unreachable, and the walk must simply stop.
        let mut data = 0u32.to_be_bytes().to_vec();
        data.extend_from_slice(b"free");
        data.extend(vec![0u8; 16]);
        let mut f = Cursor::new(data);

        assert!(survey(&mut f).is_ok());
    }

    #[test]
    fn a_box_claiming_less_than_its_header_does_not_loop_forever() {
        // A size of 4 is smaller than the 8-byte header: malformed. The walk
        // must stop instead of advancing by zero forever.
        let mut data = 4u32.to_be_bytes().to_vec();
        data.extend_from_slice(b"junk");
        data.extend(vec![0u8; 32]);
        let mut f = Cursor::new(data);

        let s = survey(&mut f).expect("must terminate");
        assert!(s.brands.is_empty());
    }

    #[test]
    fn a_box_overrunning_the_file_is_clamped_not_trusted() {
        let mut data = 0xffff_ff00u32.to_be_bytes().to_vec();
        data.extend_from_slice(b"moov");
        data.extend(vec![0u8; 16]);
        let mut f = Cursor::new(data);

        assert!(survey(&mut f).is_ok());
    }

    #[test]
    fn a_truncated_tkhd_yields_no_rotation_rather_than_an_error() {
        // Header present, matrix cut off.
        let short = vec![0u8; 20];
        let moov = bx(b"trak", &bx(b"tkhd", &short));
        let mut f = file(&[bx(b"moov", &moov)]);

        let s = survey(&mut f).expect("survey");
        assert_eq!(s.rotation(0), Rotation::None);
    }

    #[test]
    fn an_empty_file_surveys_to_nothing() {
        let mut f = Cursor::new(Vec::new());
        assert_eq!(survey(&mut f).expect("survey"), BoxSurvey::default());
    }

    #[test]
    fn deeply_nested_containers_stop_at_the_depth_limit() {
        // Nest moov > trak > mdia > trak > ... far past MAX_DEPTH. The walk
        // must return rather than recurse until the stack gives out.
        let mut inner = bx(b"mvhd", &mvhd_v0(600, 100));
        for _ in 0..64 {
            inner = bx(b"trak", &inner);
        }
        let mut f = file(&[bx(b"moov", &inner)]);
        assert!(survey(&mut f).is_ok());
    }
}
