#![allow(dead_code)]

use std::io::Cursor;

use lewton::inside_ogg::OggStreamReader;

use crate::error::SoundFontError;
use crate::sample_header::SampleHeader;

/// Number of zero-valued guard frames appended after each decoded sample.
///
/// The SoundFont specification requires at least 46 zero sample-points after
/// each sample. They keep the oscillator's `data[index + 1]` interpolation
/// reads and the loader's sanity check in bounds, and stop one sample's tail
/// from reading into the next.
const GUARD_FRAMES: usize = 46;

/// Decodes the Ogg Vorbis sample streams of a SoundFont3 `smpl` chunk into a
/// single 16-bit PCM pool and rewrites each sample header to index that pool.
pub(crate) fn decode_vorbis_samples(
    smpl: &[u8],
    sample_headers: &mut [SampleHeader],
) -> Result<Vec<i16>, SoundFontError> {
    let mut wave_data: Vec<i16> = Vec::new();

    for header in sample_headers.iter_mut() {
        // In SF3, start/end are byte offsets of this sample's Ogg stream.
        let start = header.start;
        let end = header.end;
        if start < 0 || end < start || end as usize > smpl.len() {
            return Err(SoundFontError::Sf3DecodeFailed(format!(
                "sample '{}' has an invalid byte range {start}..{end}",
                header.name
            )));
        }

        let pcm = decode_mono_stream(&smpl[start as usize..end as usize])?;

        let offset = wave_data.len() as i32;
        let frames = pcm.len() as i32;

        wave_data.extend_from_slice(&pcm);
        wave_data.resize(wave_data.len() + GUARD_FRAMES, 0);

        // Rewrite the header to index the decoded pool. start_loop/end_loop were
        // sample-frame indices into this sample's own decoded PCM.
        header.start = offset;
        header.end = offset + frames;
        header.start_loop += offset;
        header.end_loop += offset;
    }

    Ok(wave_data)
}

fn decode_mono_stream(stream: &[u8]) -> Result<Vec<i16>, SoundFontError> {
    let mut reader = OggStreamReader::new(Cursor::new(stream))
        .map_err(|e| SoundFontError::Sf3DecodeFailed(e.to_string()))?;

    if reader.ident_hdr.audio_channels != 1 {
        return Err(SoundFontError::Sf3DecodeFailed(format!(
            "expected a mono sample, but found {} channels",
            reader.ident_hdr.audio_channels
        )));
    }

    let mut pcm: Vec<i16> = Vec::new();
    while let Some(packet) = reader
        .read_dec_packet()
        .map_err(|e| SoundFontError::Sf3DecodeFailed(e.to_string()))?
    {
        // Mono: channel 0 is the whole sample.
        pcm.extend_from_slice(&packet[0]);
    }

    Ok(pcm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn u32_le(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes([
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ])
    }

    fn find_chunk(bytes: &[u8], id: &[u8]) -> usize {
        bytes
            .windows(id.len())
            .position(|w| w == id)
            .expect("chunk id not found in dummy.sf3")
    }

    #[test]
    fn decodes_first_sample_of_dummy_sf3() {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
        path.push("samples");
        path.push("dummy.sf3");
        let bytes = fs::read(&path).unwrap();

        // smpl sub-chunk: 4-byte id, 4-byte LE size, then the raw Ogg stream(s).
        let smpl_pos = find_chunk(&bytes, b"smpl");
        let smpl_size = u32_le(&bytes, smpl_pos + 4) as usize;
        let smpl = &bytes[smpl_pos + 8..smpl_pos + 8 + smpl_size];

        // shdr sub-chunk: 46-byte SampleHeader records.
        // dwStart @ +20, dwEnd @ +24, dwStartloop @ +28, dwEndloop @ +32.
        let shdr_pos = find_chunk(&bytes, b"shdr");
        let rec = shdr_pos + 8;
        let start = u32_le(&bytes, rec + 20) as i32;
        let end = u32_le(&bytes, rec + 24) as i32;
        let start_loop = u32_le(&bytes, rec + 28) as i32;
        let end_loop = u32_le(&bytes, rec + 32) as i32;

        let mut headers = vec![SampleHeader {
            name: String::from("dummy"),
            start,
            end,
            start_loop,
            end_loop,
            sample_rate: 44100,
            original_pitch: 60,
            pitch_correction: 0,
            link: 0,
            sample_type: 1,
        }];

        let wave = decode_vorbis_samples(smpl, &mut headers).unwrap();

        assert!(!wave.is_empty(), "decoded PCM pool should not be empty");
        assert_eq!(headers[0].start, 0, "first sample starts at pool offset 0");
        assert!(headers[0].end > 0);
        assert!(headers[0].end as usize <= wave.len());
        assert!(headers[0].end_loop as usize <= wave.len());
    }
}
