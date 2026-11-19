//! Deflate: the whole file as one stream, the smallest a pure-Rust build can make it; or a line at a
//! time, each leaning on the lines above it as deflate's preset dictionary, so an edit changes its
//! own line and the few below, and the file still diffs.

use super::{Body, Line, Squeezed};
use crate::container::Folded;
use crate::error::{FoldError, UnfoldError};
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

pub(crate) fn squeeze(input: &[u8], lean: Option<usize>) -> Result<Squeezed, FoldError> {
    let Some(lean) = lean else {
        let body = Body::Stream(deflate(input, &[])?);
        return Ok(Squeezed {
            options: super::whole(input),
            tables: super::Tables::Lines(Vec::new()),
            body,
        });
    };
    let lines: Vec<&[u8]> = input.split(|&byte| byte == b'\n').collect();
    let mut body = Vec::with_capacity(lines.len());
    for (at, line) in lines.iter().enumerate() {
        body.push(match line.is_empty() {
            true => Line::Empty,
            false => Line::Bytes(deflate(line, &dictionary(&lines[at.saturating_sub(lean)..at]))?),
        });
    }
    Ok(Squeezed {
        options: vec![format!("lean={lean}")],
        tables: super::Tables::Lines(Vec::new()),
        body: Body::Lines(body),
    })
}

pub(crate) fn unsqueeze(folded: &Folded<'_>) -> Result<Vec<u8>, UnfoldError> {
    if folded.header.has("whole") {
        let stream = super::stream_of(folded)?;
        let line = folded.body.first().copied().expect("a body has a line");
        let unfolded = inflate(&stream, &[], Some(super::size(folded)?));
        return super::sized(folded, unfolded.map_err(|problem| super::corrupt(&line, &problem))?);
    }
    let lean: usize =
        folded.header.value("lean").and_then(|lean| lean.parse().ok()).ok_or_else(|| {
            UnfoldError::Header("deflate a line at a time needs `lean=<lines>`".into())
        })?;
    let mut lines: Vec<Vec<u8>> = Vec::with_capacity(folded.body.len());
    for line in &folded.body {
        let unfolded = match line.text.is_empty() {
            true => Vec::new(),
            false => {
                let bytes = super::bytes_of(folded, line)?;
                let above: Vec<&[u8]> =
                    lines[lines.len().saturating_sub(lean)..].iter().map(Vec::as_slice).collect();
                inflate(&bytes, &dictionary(&above), None)
                    .map_err(|problem| super::corrupt(line, &problem))?
            }
        };
        lines.push(unfolded);
    }
    Ok(lines.join(&b'\n'))
}

/// The lines above, each with its newline, as the dictionary for the next one.
fn dictionary(above: &[&[u8]]) -> Vec<u8> {
    above.iter().flat_map(|line| line.iter().chain(b"\n").copied()).collect()
}

/// Raw deflate (no zlib header), at the best compression.
pub(crate) fn deflate(data: &[u8], dictionary: &[u8]) -> Result<Vec<u8>, FoldError> {
    let mut compress = Compress::new(Compression::best(), false);
    if !dictionary.is_empty() {
        compress
            .set_dictionary(dictionary)
            .map_err(|error| FoldError::Compressor(error.to_string()))?;
    }
    let mut out = Vec::with_capacity(data.len() / 2 + 64);
    loop {
        let input = &data[compress.total_in() as usize..];
        let status = compress
            .compress_vec(input, &mut out, FlushCompress::Finish)
            .map_err(|error| FoldError::Compressor(error.to_string()))?;
        if status == Status::StreamEnd {
            return Ok(out);
        }
        out.reserve(out.capacity().max(1024));
    }
}

/// `data` inflated, refused once it passes `limit` bytes.
pub(crate) fn inflate(
    data: &[u8],
    dictionary: &[u8],
    limit: Option<usize>,
) -> Result<Vec<u8>, String> {
    let mut decompress = Decompress::new(false);
    if !dictionary.is_empty() {
        decompress.set_dictionary(dictionary).map_err(|error| error.to_string())?;
    }
    let mut out = Vec::with_capacity(data.len() * 3 + 64);
    loop {
        let (before_in, before_out) = (decompress.total_in(), decompress.total_out());
        let input = &data[decompress.total_in() as usize..];
        let status = decompress
            .decompress_vec(input, &mut out, FlushDecompress::Finish)
            .map_err(|error| format!("this deflate doesn't inflate: {error}"))?;
        if limit.is_some_and(|limit| out.len() > limit) {
            return Err("this deflate inflates past the size it was given".into());
        }
        match status {
            Status::StreamEnd if decompress.total_in() as usize == data.len() => return Ok(out),
            Status::StreamEnd => return Err("bytes follow the end of the deflate stream".into()),
            _ if out.len() == out.capacity() => {
                let room = limit.map_or(usize::MAX, |limit| limit + 1 - out.len().min(limit));
                out.reserve(out.capacity().max(4096).min(room.max(1)));
            }
            _ if (decompress.total_in(), decompress.total_out()) == (before_in, before_out) => {
                return Err("the deflate stream is cut short".into());
            }
            _ => {}
        }
    }
}

/// What git stores for `data`: zlib at git's default level, header and all.
pub(crate) fn git_size(data: &[u8]) -> usize {
    let mut compress = Compress::new(Compression::new(6), true);
    let mut out = Vec::with_capacity(data.len() / 2 + 64);
    loop {
        let input = &data[compress.total_in() as usize..];
        match compress.compress_vec(input, &mut out, FlushCompress::Finish) {
            Ok(Status::StreamEnd) | Err(_) => return out.len(),
            Ok(_) => out.reserve(out.capacity().max(1024)),
        }
    }
}
