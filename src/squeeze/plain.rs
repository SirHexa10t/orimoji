//! No squeeze at all: the bytes as they are, folded a line at a time or as one stream.

use super::{Body, Line, Squeezed};
use crate::container::Folded;
use crate::error::UnfoldError;

pub(crate) fn squeeze(input: &[u8], whole: bool) -> Squeezed {
    let body = match whole {
        true => Body::Stream(input.to_vec()),
        false => Body::Lines(
            input
                .split(|&byte| byte == b'\n')
                .map(|line| match line.is_empty() {
                    true => Line::Empty,
                    false => Line::Bytes(line.to_vec()),
                })
                .collect(),
        ),
    };
    let options = match whole {
        true => super::whole(input),
        false => Vec::new(),
    };
    Squeezed { options, tables: Vec::new(), body }
}

pub(crate) fn unsqueeze(folded: &Folded<'_>) -> Result<Vec<u8>, UnfoldError> {
    match folded.header.has("whole") {
        true => super::sized(folded, super::stream_of(folded)?),
        false => super::lines_of(folded, |_, bytes| Ok(bytes)),
    }
}
