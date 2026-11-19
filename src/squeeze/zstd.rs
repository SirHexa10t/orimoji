//! Zstandard over the whole file: the smallest whole-file squeeze, and the fastest to unfold.

use super::Squeezed;
use crate::container::Folded;
use crate::error::{FoldError, UnfoldError};

#[cfg(feature = "zstd")]
pub(crate) fn squeeze(input: &[u8], level: i32) -> Result<Squeezed, FoldError> {
    let packed = zstd::bulk::compress(input, level)
        .map_err(|error| FoldError::Compressor(error.to_string()))?;
    Ok(Squeezed {
        options: super::whole(input),
        tables: super::Tables::Lines(Vec::new()),
        body: super::Body::Stream(packed),
    })
}

#[cfg(not(feature = "zstd"))]
pub(crate) fn squeeze(_: &[u8], _: i32) -> Result<Squeezed, FoldError> {
    Err(FoldError::NoZstd)
}

#[cfg(feature = "zstd")]
pub(crate) fn unsqueeze(folded: &Folded<'_>) -> Result<Vec<u8>, UnfoldError> {
    let stream = super::stream_of(folded)?;
    let line = folded.body.first().copied().expect("a body has a line");
    // Room for exactly the size the header names, so a damaged stream can't ask for more.
    let unfolded = zstd::bulk::decompress(&stream, super::size(folded)?).map_err(|error| {
        super::corrupt(&line, &format!("this zstd doesn't decompress: {error}"))
    })?;
    super::sized(folded, unfolded)
}

#[cfg(not(feature = "zstd"))]
pub(crate) fn unsqueeze(_: &Folded<'_>) -> Result<Vec<u8>, UnfoldError> {
    Err(UnfoldError::NoZstd)
}
