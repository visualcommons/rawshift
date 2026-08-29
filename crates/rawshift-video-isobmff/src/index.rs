//! The sample index: turning a track's sample tables into a flat, ordered list
//! of samples with byte ranges and timestamps.
//!
//! ISOBMFF does not store a sample table. It stores five or six *compressed*
//! tables — sizes, chunk offsets, samples-per-chunk runs, decode-time runs,
//! composition offsets, and sync sample numbers — that have to be walked
//! together to recover where each sample lives and when it is shown. That
//! reconstruction is what this module does, and it is the part `mp4-atom`
//! deliberately leaves to its caller.
//!
//! Every table is attacker-controlled, so the walk is total: an inconsistent
//! table truncates the index rather than panicking or producing a byte range
//! that points outside the file.

use mp4_atom::{Edts, Stbl, StszSamples};

/// One sample: where it is, when it decodes, and when it is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    /// Byte offset from the start of the file.
    pub offset: u64,
    /// Size in bytes.
    pub size: u32,
    /// Decode timestamp, in the track's timescale.
    pub dts: i64,
    /// Presentation timestamp, in the track's timescale.
    ///
    /// Equal to `dts` unless a `ctts` composition offset applies, which is how
    /// reordered streams say a picture is decoded before it is shown.
    pub pts: i64,
    /// How long the sample occupies, in the track's timescale.
    pub duration: u32,
    /// Whether the container marks this as a sync sample.
    ///
    /// Advisory only: rawshift derives random access from the bitstream, which
    /// is authoritative. Kept because it is what seeking indexes against.
    pub is_sync: bool,
}

/// A track's samples, in decode order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SampleIndex {
    samples: Vec<Sample>,
}

impl SampleIndex {
    /// Reconstruct the index from a track's sample tables, applying .
    ///
    /// Never fails: a malformed or inconsistent table stops the walk and
    /// yields the samples recovered so far, because a partly-readable track is
    /// more useful than none and the caller cannot act on a parse error here
    /// anyway.
    #[must_use]
    pub fn build(stbl: &Stbl, edts: Option<&Edts>) -> Self {
        let shift = presentation_shift(edts);
        let sizes = sample_sizes(&stbl.stsz.samples);
        let sample_count = sizes.len();
        if sample_count == 0 {
            return Self::default();
        }

        let chunk_offsets: Vec<u64> = match (&stbl.stco, &stbl.co64) {
            // co64 wins when both are present: a file large enough to need
            // 64-bit offsets has a stco that cannot address it.
            (_, Some(co64)) => co64.entries.clone(),
            (Some(stco), None) => stco.entries.iter().map(|&o| u64::from(o)).collect(),
            (None, None) => return Self::default(),
        };

        let offsets = sample_offsets(&stbl.stsc.entries, &chunk_offsets, &sizes);
        let times = sample_times(&stbl.stts.entries, sample_count);
        let composition = composition_offsets(stbl.ctts.as_ref(), sample_count);

        // stss lists the sync samples by 1-based number. Its *absence* means
        // every sample is a sync sample — the convention for all-intra tracks —
        // which is the opposite of treating an absent table as "none".
        let sync: Option<std::collections::HashSet<u32>> = stbl
            .stss
            .as_ref()
            .map(|stss| stss.entries.iter().copied().collect());

        let usable = offsets.len().min(times.len()).min(sample_count);
        let samples = (0..usable)
            .map(|i| {
                let (dts, duration) = times[i];
                let composition_shift = composition.get(i).copied().unwrap_or(0);
                Sample {
                    offset: offsets[i],
                    size: sizes[i],
                    dts: dts.saturating_sub(shift),
                    pts: dts.saturating_add(composition_shift).saturating_sub(shift),
                    duration,
                    is_sync: sync
                        .as_ref()
                        .is_none_or(|set| set.contains(&(i as u32 + 1))),
                }
            })
            .collect();

        Self { samples }
    }

    /// The samples, in decode order.
    #[must_use]
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// How many samples the track has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether the track has no samples.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The index of the last sync sample at or before `timestamp` in
    /// presentation order, or the first sample when none precedes it.
    ///
    /// This is the seek landing point: decoding can only resume at a random
    /// access point, so a request between two of them lands on the earlier
    /// one and the caller decodes forward.
    #[must_use]
    pub fn sync_sample_at_or_before(&self, timestamp: i64) -> Option<usize> {
        if self.samples.is_empty() {
            return None;
        }
        let landing = self
            .samples
            .iter()
            .enumerate()
            .rfind(|(_, s)| s.is_sync && s.pts <= timestamp)
            .map(|(i, _)| i);

        // Before the first sync sample there is nothing to resume from, so the
        // start of the track is the only honest answer.
        Some(landing.unwrap_or(0))
    }
}

/// How far to shift the timeline so presentation starts at zero.
///
/// An `elst` maps presentation time onto media time. Encoders that reorder
/// write a single edit whose `media_time` equals the composition delay, which
/// says "presentation begins this far into the media" — the first frame is
/// shown at time zero even though its composition timestamp is not. Ignoring
/// it leaves every timestamp offset by the reorder delay, disagreeing with
/// every other tool that reads the file.
///
/// Only the single-entry case is honoured, which is what encoders write. A
/// multi-entry edit list is a genuine cut list that would have to trim and
/// reorder samples; that is out of scope, and ignoring it yields untrimmed
/// media rather than wrong media.
fn presentation_shift(edts: Option<&Edts>) -> i64 {
    let Some(elst) = edts.and_then(|e| e.elst.as_ref()) else {
        return 0;
    };
    let [entry] = elst.entries.as_slice() else {
        return 0;
    };
    // An empty edit (media_time == -1, spelled None here) is a dwell with no
    // media and shifts nothing.
    let Some(media_time) = entry.media_time else {
        return 0;
    };
    i64::try_from(media_time).unwrap_or(0)
}

/// Expand `stsz` into a per-sample size list.
fn sample_sizes(samples: &StszSamples) -> Vec<u32> {
    match samples {
        StszSamples::Identical { count, size } => vec![*size; *count as usize],
        StszSamples::Different { sizes } => sizes.clone(),
    }
}

/// Walk `stsc` and the chunk offsets to place every sample in the file.
///
/// `stsc` stores runs: an entry `(first_chunk, samples_per_chunk, ..)` applies
/// from `first_chunk` until the next entry's `first_chunk`. Within a chunk,
/// samples are laid out back to back from the chunk's offset.
fn sample_offsets(stsc: &[mp4_atom::StscEntry], chunk_offsets: &[u64], sizes: &[u32]) -> Vec<u64> {
    let mut offsets = Vec::with_capacity(sizes.len());
    let mut sample = 0usize;

    for (index, entry) in stsc.iter().enumerate() {
        // Chunk numbers are 1-based; a zero would underflow the range below.
        if entry.first_chunk == 0 {
            break;
        }
        let first = entry.first_chunk as usize - 1;
        let last = stsc
            .get(index + 1)
            .map_or(chunk_offsets.len(), |next| {
                (next.first_chunk as usize).saturating_sub(1)
            })
            .min(chunk_offsets.len());

        for chunk in first..last {
            let Some(&base) = chunk_offsets.get(chunk) else {
                return offsets;
            };
            let mut cursor = base;
            for _ in 0..entry.samples_per_chunk {
                let Some(&size) = sizes.get(sample) else {
                    return offsets;
                };
                offsets.push(cursor);
                // A crafted table can push the cursor past the file; the
                // demuxer bounds-checks the read, so saturating here is enough
                // to keep the walk total.
                cursor = cursor.saturating_add(u64::from(size));
                sample += 1;
            }
        }
    }

    offsets
}

/// Expand the `stts` runs into per-sample `(dts, duration)`.
fn sample_times(stts: &[mp4_atom::SttsEntry], sample_count: usize) -> Vec<(i64, u32)> {
    let mut times = Vec::with_capacity(sample_count);
    let mut dts: i64 = 0;

    for entry in stts {
        for _ in 0..entry.sample_count {
            if times.len() >= sample_count {
                return times;
            }
            times.push((dts, entry.sample_delta));
            dts = dts.saturating_add(i64::from(entry.sample_delta));
        }
    }

    // A track whose stts covers fewer samples than stsz claims is malformed.
    // Extend with zero-duration entries at the last timestamp rather than
    // dropping the samples: their bytes are still readable.
    while times.len() < sample_count {
        times.push((dts, 0));
    }
    times
}

/// Expand the `ctts` runs into per-sample composition offsets.
fn composition_offsets(ctts: Option<&mp4_atom::Ctts>, sample_count: usize) -> Vec<i64> {
    let Some(ctts) = ctts else {
        return vec![0; sample_count];
    };

    let mut offsets = Vec::with_capacity(sample_count);
    for entry in &ctts.entries {
        for _ in 0..entry.sample_count {
            if offsets.len() >= sample_count {
                return offsets;
            }
            offsets.push(entry.sample_offset);
        }
    }
    offsets.resize(sample_count, 0);
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp4_atom::{
        Co64, Ctts, CttsEntry, Elst, ElstEntry, Stco, Stsc, StscEntry, Stsd, Stss, Stsz, Stts,
        SttsEntry,
    };

    /// A minimal `stbl` from the parts a test cares about.
    fn stbl(
        sizes: StszSamples,
        stsc: Vec<StscEntry>,
        chunks: Vec<u32>,
        stts: Vec<SttsEntry>,
        ctts: Option<Vec<CttsEntry>>,
        stss: Option<Vec<u32>>,
    ) -> Stbl {
        Stbl {
            stsd: Stsd { codecs: vec![] },
            stts: Stts { entries: stts },
            ctts: ctts.map(|entries| Ctts { entries }),
            stss: stss.map(|entries| Stss { entries }),
            stsc: Stsc { entries: stsc },
            stsz: Stsz { samples: sizes },
            stco: Some(Stco { entries: chunks }),
            co64: None,
            sbgp: vec![],
            sgpd: vec![],
            subs: vec![],
            saiz: vec![],
            saio: vec![],
            cslg: None,
        }
    }

    fn run(count: u32, delta: u32) -> SttsEntry {
        SttsEntry {
            sample_count: count,
            sample_delta: delta,
        }
    }

    #[test]
    fn samples_are_laid_out_back_to_back_within_a_chunk() {
        // Two chunks of two samples each, at offsets 100 and 500.
        let index = SampleIndex::build(
            &stbl(
                StszSamples::Different {
                    sizes: vec![10, 20, 30, 40],
                },
                vec![StscEntry {
                    first_chunk: 1,
                    samples_per_chunk: 2,
                    sample_description_index: 1,
                }],
                vec![100, 500],
                vec![run(4, 512)],
                None,
                None,
            ),
            None,
        );

        let offsets: Vec<_> = index.samples().iter().map(|s| s.offset).collect();
        assert_eq!(offsets, vec![100, 110, 500, 530]);
        let sizes: Vec<_> = index.samples().iter().map(|s| s.size).collect();
        assert_eq!(sizes, vec![10, 20, 30, 40]);
    }

    #[test]
    fn a_second_stsc_run_changes_the_samples_per_chunk() {
        // Chunk 1 holds one sample; chunks 2 and 3 hold two each.
        let index = SampleIndex::build(
            &stbl(
                StszSamples::Identical { count: 5, size: 10 },
                vec![
                    StscEntry {
                        first_chunk: 1,
                        samples_per_chunk: 1,
                        sample_description_index: 1,
                    },
                    StscEntry {
                        first_chunk: 2,
                        samples_per_chunk: 2,
                        sample_description_index: 1,
                    },
                ],
                vec![1000, 2000, 3000],
                vec![run(5, 100)],
                None,
                None,
            ),
            None,
        );

        let offsets: Vec<_> = index.samples().iter().map(|s| s.offset).collect();
        assert_eq!(offsets, vec![1000, 2000, 2010, 3000, 3010]);
    }

    #[test]
    fn decode_timestamps_accumulate_across_stts_runs() {
        let index = SampleIndex::build(
            &stbl(
                StszSamples::Identical { count: 5, size: 1 },
                vec![StscEntry {
                    first_chunk: 1,
                    samples_per_chunk: 5,
                    sample_description_index: 1,
                }],
                vec![0],
                // Three samples of 100, then two of 50.
                vec![run(3, 100), run(2, 50)],
                None,
                None,
            ),
            None,
        );

        let dts: Vec<_> = index.samples().iter().map(|s| s.dts).collect();
        assert_eq!(dts, vec![0, 100, 200, 300, 350]);
        let durations: Vec<_> = index.samples().iter().map(|s| s.duration).collect();
        assert_eq!(durations, vec![100, 100, 100, 50, 50]);
    }

    #[test]
    fn composition_offsets_separate_presentation_from_decode_order() {
        // The classic reordered pattern: an I frame, a P frame shown last, and
        // two B frames shown in between.
        let index = SampleIndex::build(
            &stbl(
                StszSamples::Identical { count: 4, size: 1 },
                vec![StscEntry {
                    first_chunk: 1,
                    samples_per_chunk: 4,
                    sample_description_index: 1,
                }],
                vec![0],
                vec![run(4, 100)],
                Some(vec![
                    CttsEntry {
                        sample_count: 1,
                        sample_offset: 0,
                    },
                    CttsEntry {
                        sample_count: 1,
                        sample_offset: 300,
                    },
                    CttsEntry {
                        sample_count: 2,
                        sample_offset: 0,
                    },
                ]),
                None,
            ),
            None,
        );

        let dts: Vec<_> = index.samples().iter().map(|s| s.dts).collect();
        let pts: Vec<_> = index.samples().iter().map(|s| s.pts).collect();
        assert_eq!(dts, vec![0, 100, 200, 300]);
        // The second sample decodes at 100 but is shown at 400, after the two
        // B frames at 200 and 300.
        assert_eq!(pts, vec![0, 400, 200, 300]);
    }

    #[test]
    fn a_negative_composition_offset_is_honoured() {
        // Version 1 ctts allows negative offsets, which move presentation
        // earlier than decode.
        let index = SampleIndex::build(
            &stbl(
                StszSamples::Identical { count: 2, size: 1 },
                vec![StscEntry {
                    first_chunk: 1,
                    samples_per_chunk: 2,
                    sample_description_index: 1,
                }],
                vec![0],
                vec![run(2, 100)],
                Some(vec![CttsEntry {
                    sample_count: 2,
                    sample_offset: -100,
                }]),
                None,
            ),
            None,
        );

        let pts: Vec<_> = index.samples().iter().map(|s| s.pts).collect();
        assert_eq!(pts, vec![-100, 0]);
    }

    #[test]
    fn an_absent_stss_means_every_sample_is_a_sync_sample() {
        // The all-intra convention. Reading an absent table as "no sync
        // samples" would make such a track unseekable.
        let index = SampleIndex::build(
            &stbl(
                StszSamples::Identical { count: 3, size: 1 },
                vec![StscEntry {
                    first_chunk: 1,
                    samples_per_chunk: 3,
                    sample_description_index: 1,
                }],
                vec![0],
                vec![run(3, 100)],
                None,
                None,
            ),
            None,
        );

        assert!(index.samples().iter().all(|s| s.is_sync));
    }

    #[test]
    fn stss_marks_sync_samples_by_one_based_number() {
        let index = SampleIndex::build(
            &stbl(
                StszSamples::Identical { count: 5, size: 1 },
                vec![StscEntry {
                    first_chunk: 1,
                    samples_per_chunk: 5,
                    sample_description_index: 1,
                }],
                vec![0],
                vec![run(5, 100)],
                None,
                // Samples 1 and 4, one-based: indices 0 and 3.
                Some(vec![1, 4]),
            ),
            None,
        );

        let sync: Vec<_> = index.samples().iter().map(|s| s.is_sync).collect();
        assert_eq!(sync, vec![true, false, false, true, false]);
    }

    #[test]
    fn co64_wins_over_stco_when_both_are_present() {
        // A file large enough to need 64-bit offsets has an stco that cannot
        // address it, so preferring stco would place samples wrongly.
        let mut tables = stbl(
            StszSamples::Identical { count: 1, size: 4 },
            vec![StscEntry {
                first_chunk: 1,
                samples_per_chunk: 1,
                sample_description_index: 1,
            }],
            vec![42],
            vec![run(1, 100)],
            None,
            None,
        );
        tables.co64 = Some(Co64 {
            entries: vec![0x1_0000_0000],
        });

        let index = SampleIndex::build(&tables, None);
        assert_eq!(index.samples()[0].offset, 0x1_0000_0000);
    }

    #[test]
    fn seeking_lands_on_the_sync_sample_at_or_before_the_request() {
        let index = SampleIndex::build(
            &stbl(
                StszSamples::Identical { count: 6, size: 1 },
                vec![StscEntry {
                    first_chunk: 1,
                    samples_per_chunk: 6,
                    sample_description_index: 1,
                }],
                vec![0],
                vec![run(6, 100)],
                None,
                Some(vec![1, 4]), // sync at pts 0 and 300
            ),
            None,
        );

        // Exactly on a sync sample.
        assert_eq!(index.sync_sample_at_or_before(300), Some(3));
        // Between two: must land on the earlier one, never the later.
        assert_eq!(index.sync_sample_at_or_before(250), Some(0));
        assert_eq!(index.sync_sample_at_or_before(450), Some(3));
        // Past the end: the last sync sample.
        assert_eq!(index.sync_sample_at_or_before(99_999), Some(3));
        // Before the first: the start of the track is the only resumable point.
        assert_eq!(index.sync_sample_at_or_before(-100), Some(0));
    }

    fn edts(media_time: Option<u64>) -> Edts {
        Edts {
            elst: Some(Elst {
                entries: vec![ElstEntry {
                    segment_duration: 1_000,
                    media_time,
                    media_rate: 1.into(),
                }],
            }),
        }
    }

    fn reordered_tables() -> Stbl {
        // Two frames with a composition delay of 100, as a reordering encoder
        // writes: composition timestamps start at 100, not 0.
        stbl(
            StszSamples::Identical { count: 2, size: 1 },
            vec![StscEntry {
                first_chunk: 1,
                samples_per_chunk: 2,
                sample_description_index: 1,
            }],
            vec![0],
            vec![run(2, 100)],
            Some(vec![CttsEntry {
                sample_count: 2,
                sample_offset: 100,
            }]),
            None,
        )
    }

    #[test]
    fn an_edit_list_shifts_presentation_to_start_at_zero() {
        // Without the edit list every timestamp is offset by the reorder
        // delay, and rawshift would disagree with every other tool reading the
        // same file.
        let unshifted = SampleIndex::build(&reordered_tables(), None);
        assert_eq!(unshifted.samples()[0].pts, 100);

        let shifted = SampleIndex::build(&reordered_tables(), Some(&edts(Some(100))));
        assert_eq!(shifted.samples()[0].pts, 0, "presentation must start at 0");
        // Decode time goes negative, which is correct: the frame is decoded
        // before the presentation timeline begins.
        assert_eq!(shifted.samples()[0].dts, -100);
    }

    #[test]
    fn an_empty_edit_shifts_nothing() {
        // media_time == -1 is a dwell with no media, not an offset.
        let index = SampleIndex::build(&reordered_tables(), Some(&edts(None)));
        assert_eq!(index.samples()[0].pts, 100);
    }

    #[test]
    fn a_multi_entry_edit_list_is_ignored_rather_than_half_applied() {
        // A real cut list would have to trim and reorder samples. Ignoring it
        // yields untrimmed media, which is recoverable; applying only its
        // first entry would yield wrong media, which is not.
        let cut_list = Edts {
            elst: Some(Elst {
                entries: vec![
                    ElstEntry {
                        segment_duration: 500,
                        media_time: Some(100),
                        media_rate: 1.into(),
                    },
                    ElstEntry {
                        segment_duration: 500,
                        media_time: Some(900),
                        media_rate: 1.into(),
                    },
                ],
            }),
        };
        let index = SampleIndex::build(&reordered_tables(), Some(&cut_list));
        assert_eq!(index.samples()[0].pts, 100, "left untrimmed, not shifted");
    }

    #[test]
    fn seeking_an_empty_index_yields_nothing() {
        assert_eq!(SampleIndex::default().sync_sample_at_or_before(0), None);
        assert!(SampleIndex::default().is_empty());
    }

    #[test]
    fn a_chunk_offset_table_shorter_than_stsc_claims_truncates_cleanly() {
        // stsc describes three chunks; stco lists one. The walk must stop at
        // what it can place rather than indexing past the table.
        let index = SampleIndex::build(
            &stbl(
                StszSamples::Identical { count: 6, size: 10 },
                vec![StscEntry {
                    first_chunk: 1,
                    samples_per_chunk: 2,
                    sample_description_index: 1,
                }],
                vec![100],
                vec![run(6, 100)],
                None,
                None,
            ),
            None,
        );

        assert_eq!(index.len(), 2, "only the samples in the known chunk");
    }

    #[test]
    fn a_zero_first_chunk_does_not_underflow() {
        // first_chunk is 1-based; a zero is malformed and must not wrap.
        let index = SampleIndex::build(
            &stbl(
                StszSamples::Identical { count: 2, size: 10 },
                vec![StscEntry {
                    first_chunk: 0,
                    samples_per_chunk: 2,
                    sample_description_index: 1,
                }],
                vec![100],
                vec![run(2, 100)],
                None,
                None,
            ),
            None,
        );
        assert!(index.is_empty());
    }

    #[test]
    fn a_track_with_no_chunk_offsets_yields_no_samples() {
        let mut tables = stbl(
            StszSamples::Identical { count: 4, size: 10 },
            vec![StscEntry {
                first_chunk: 1,
                samples_per_chunk: 2,
                sample_description_index: 1,
            }],
            vec![],
            vec![run(4, 100)],
            None,
            None,
        );
        tables.stco = None;
        assert!(SampleIndex::build(&tables, None).is_empty());
    }

    #[test]
    fn an_stts_shorter_than_stsz_still_yields_every_sample() {
        // Malformed, but the sample bytes are readable, so they are kept with
        // the last known timestamp rather than dropped.
        let index = SampleIndex::build(
            &stbl(
                StszSamples::Identical { count: 4, size: 10 },
                vec![StscEntry {
                    first_chunk: 1,
                    samples_per_chunk: 4,
                    sample_description_index: 1,
                }],
                vec![0],
                vec![run(2, 100)],
                None,
                None,
            ),
            None,
        );

        assert_eq!(index.len(), 4);
        assert_eq!(index.samples()[3].dts, 200);
        assert_eq!(index.samples()[3].duration, 0);
    }

    #[test]
    fn an_identical_size_table_expands_to_every_sample() {
        let index = SampleIndex::build(
            &stbl(
                StszSamples::Identical {
                    count: 3,
                    size: 1234,
                },
                vec![StscEntry {
                    first_chunk: 1,
                    samples_per_chunk: 3,
                    sample_description_index: 1,
                }],
                vec![0],
                vec![run(3, 100)],
                None,
                None,
            ),
            None,
        );

        assert_eq!(index.len(), 3);
        assert!(index.samples().iter().all(|s| s.size == 1234));
    }
}
