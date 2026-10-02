//! Dropping one exact sequence from the very start of a stream.
//!
//! Plain `Read` logic with nothing platform-specific in it, so it is built
//! and tested everywhere, though only the Windows spawn uses it.

use std::io::Read;

/// A reader that drops `prefix` once, if the stream begins with exactly it,
/// and passes every other byte through unchanged.
///
/// Bytes that start out matching are held back until the match is decided.
/// When it fails, or the stream ends before it completes, they are handed
/// out as read, so nothing but the whole prefix is ever lost.
pub(crate) struct SkipLeading<R> {
    inner: R,
    prefix: &'static [u8],
    /// How much of `prefix` the stream has matched so far.
    matched: usize,
    /// Whether the match is decided, one way or the other.
    decided: bool,
    /// Bytes owed to the caller before anything more is read.
    pending: Vec<u8>,
}

impl<R: Read> SkipLeading<R> {
    pub(crate) fn new(inner: R, prefix: &'static [u8]) -> Self {
        Self {
            inner,
            prefix,
            matched: 0,
            decided: false,
            pending: Vec::new(),
        }
    }
}

impl<R: Read> Read for SkipLeading<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if !self.pending.is_empty() {
                let n = self.pending.len().min(buf.len());
                buf[..n].copy_from_slice(&self.pending[..n]);
                self.pending.drain(..n);
                return Ok(n);
            }
            if self.decided {
                return self.inner.read(buf);
            }

            let n = self.inner.read(buf)?;
            if n == 0 {
                // The stream ended partway through the prefix: what matched
                // was not the prefix after all, so it is owed back.
                self.decided = true;
                self.pending = self.prefix[..self.matched].to_vec();
                if self.pending.is_empty() {
                    return Ok(0);
                }
                continue;
            }

            let wanted = &self.prefix[self.matched..];
            let take = wanted.len().min(n);
            if buf[..take] != wanted[..take] {
                // Something else came first, so nothing is dropped: the held
                // bytes go out ahead of the ones just read.
                self.decided = true;
                self.pending = self.prefix[..self.matched].to_vec();
                self.pending.extend_from_slice(&buf[..n]);
                continue;
            }

            self.matched += take;
            if self.matched == self.prefix.len() {
                self.decided = true;
                if take < n {
                    buf.copy_within(take..n, 0);
                    return Ok(n - take);
                }
            }
            // All of this read was prefix; read again rather than return
            // zero, which would mean end-of-file.
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::Read;

    use super::SkipLeading;

    const QUERY: &[u8] = b"\x1b[6n";

    /// A stream that hands out its bytes in the given pieces, one per read.
    struct Pieces(VecDeque<Vec<u8>>);

    impl Read for Pieces {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let Some(mut piece) = self.0.pop_front() else {
                return Ok(0);
            };
            let n = piece.len().min(buf.len());
            buf[..n].copy_from_slice(&piece[..n]);
            if n < piece.len() {
                self.0.push_front(piece.split_off(n));
            }
            Ok(n)
        }
    }

    fn through(pieces: &[&[u8]]) -> Vec<u8> {
        let stream = Pieces(pieces.iter().map(|piece| piece.to_vec()).collect());
        let mut out = Vec::new();
        SkipLeading::new(stream, QUERY)
            .read_to_end(&mut out)
            .expect("reads");
        out
    }

    #[test]
    fn an_exact_leading_prefix_is_dropped() {
        assert_eq!(through(&[b"\x1b[6nhello"]), b"hello");
    }

    #[test]
    fn a_prefix_split_across_single_byte_reads_is_dropped() {
        let bytes = b"\x1b[6nhello";
        let pieces: Vec<&[u8]> = bytes.chunks(1).collect();
        assert_eq!(through(&pieces), b"hello");
    }

    #[test]
    fn a_stream_starting_with_something_else_passes_through_intact() {
        assert_eq!(through(&[b"\x1b[?25l\x1b[6n"]), b"\x1b[?25l\x1b[6n");
        assert_eq!(through(&[b"\x1b[", b"2J"]), b"\x1b[2J");
    }

    #[test]
    fn the_query_later_in_the_stream_passes_through() {
        assert_eq!(
            through(&[b"\x1b[6n", b"hi\x1b[6n", b"\x1b[6n"]),
            b"hi\x1b[6n\x1b[6n"
        );
    }

    #[test]
    fn a_stream_ending_partway_through_the_prefix_passes_through() {
        assert_eq!(through(&[b"\x1b[", b"6"]), b"\x1b[6");
    }

    #[test]
    fn an_empty_stream_stays_empty() {
        assert_eq!(through(&[]), b"");
    }

    #[test]
    fn a_small_buffer_still_gets_every_held_byte() {
        let stream = Pieces(VecDeque::from([b"\x1b[6".to_vec(), b"x".to_vec()]));
        let mut reader = SkipLeading::new(stream, QUERY);
        let mut out = Vec::new();
        let mut byte = [0u8; 1];
        while reader.read(&mut byte).expect("reads") == 1 {
            out.push(byte[0]);
        }
        assert_eq!(out, b"\x1b[6x");
    }
}
