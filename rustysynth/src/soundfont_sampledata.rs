#![allow(dead_code)]

use std::io::Read;

use crate::binary_reader::BinaryReader;
use crate::error::SoundFontError;
use crate::four_cc::FourCC;
use crate::read_counter::ReadCounter;

/// The sample data (`sdta`) of a SoundFont, classified by encoding.
pub(crate) enum SoundFontSampleData {
    /// SoundFont2: raw little-endian 16-bit PCM, ready for playback.
    Pcm {
        bits_per_sample: i32,
        wave_data: Vec<i16>,
    },
    /// SoundFont3: the concatenated per-sample Ogg Vorbis streams from `smpl`.
    #[cfg(feature = "sf3")]
    Vorbis(Vec<u8>),
}

impl SoundFontSampleData {
    pub(crate) fn new<R: Read>(reader: &mut R) -> Result<Self, SoundFontError> {
        let chunk_id = BinaryReader::read_four_cc(reader)?;
        if chunk_id != b"LIST" {
            return Err(SoundFontError::ListChunkNotFound);
        }

        let end = BinaryReader::read_u32(reader)? as usize;
        let reader = &mut ReadCounter::new(reader);

        let list_type = BinaryReader::read_four_cc(reader)?;
        if list_type != b"sdta" {
            return Err(SoundFontError::InvalidListChunkType {
                expected: FourCC::from_bytes(*b"sdta"),
                actual: list_type,
            });
        }

        let mut sample_data: Option<Vec<u8>> = None;

        while reader.bytes_read() < end {
            let id = BinaryReader::read_four_cc(reader)?;
            let size = BinaryReader::read_u32(reader)? as usize;

            match id.as_bytes() {
                b"smpl" => sample_data = Some(BinaryReader::read_bytes(reader, size)?),
                b"sm24" => BinaryReader::discard_data(reader, size)?,
                _ => return Err(SoundFontError::ListContainsUnknownId(id)),
            }
        }

        let Some(sample_data) = sample_data else {
            return Err(SoundFontError::SampleDataNotFound);
        };

        if sample_data.len() < 2 {
            return Err(SoundFontError::SampleDataNotFound);
        }

        // SoundFont3 stores Ogg Vorbis streams (RIFF "OggS" magic) in `smpl`.
        if sample_data.starts_with(b"OggS") {
            #[cfg(feature = "sf3")]
            return Ok(SoundFontSampleData::Vorbis(sample_data));
            #[cfg(not(feature = "sf3"))]
            return Err(SoundFontError::UnsupportedSampleFormat);
        }

        Ok(SoundFontSampleData::Pcm {
            bits_per_sample: 16,
            wave_data: bytes_to_i16(&sample_data),
        })
    }
}

/// Reinterprets a little-endian byte buffer as 16-bit PCM samples.
fn bytes_to_i16(bytes: &[u8]) -> Vec<i16> {
    bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}
