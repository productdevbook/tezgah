//! Listing, a page at a time.
//!
//! There is no unbounded list in this crate. A collection that is small in
//! development is the one that arrives with fifty thousand rows in production,
//! and by then the endpoint returning all of them is load-bearing.
//!
//! Pages are cursors rather than offsets. An offset re-reads and discards
//! everything before it, so deep pages get slower the further in they are, and
//! a row inserted while somebody is paging shifts every later page by one — so
//! they see something twice, or never see it at all.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{Error, Result};

/// Most rows anybody may ask for at once, however large a limit they send.
pub const MAX_LIMIT: u32 = 200;
pub const DEFAULT_LIMIT: u32 = 50;

/// Where the last page stopped.
///
/// Carries a timestamp and an id because a timestamp alone is not unique: two
/// rows written in the same microsecond would make the boundary ambiguous, and
/// one of them would be skipped or repeated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub at: chrono::DateTime<chrono::Utc>,
    pub id: Uuid,
}

impl Cursor {
    /// Opaque on purpose: a client that takes one apart will depend on how the
    /// ordering works, and then the ordering cannot change.
    pub fn encode(&self) -> String {
        let raw = format!("{}|{}", self.at.timestamp_micros(), self.id);
        URL_SAFE_NO_PAD.encode(raw)
    }

    pub fn decode(text: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD
            .decode(text)
            .map_err(|_| Error::invalid("that is not a cursor"))?;
        let raw = String::from_utf8(bytes).map_err(|_| Error::invalid("that is not a cursor"))?;
        let (micros, id) = raw
            .split_once('|')
            .ok_or_else(|| Error::invalid("that is not a cursor"))?;

        let micros: i64 = micros
            .parse()
            .map_err(|_| Error::invalid("that is not a cursor"))?;
        let at = chrono::DateTime::from_timestamp_micros(micros)
            .ok_or_else(|| Error::invalid("that is not a cursor"))?;
        let id = Uuid::parse_str(id).map_err(|_| Error::invalid("that is not a cursor"))?;

        Ok(Cursor { at, id })
    }
}

/// What a caller asks for.
#[derive(Debug, Clone, Copy, Default)]
pub struct Paging {
    pub after: Option<Cursor>,
    limit: Option<u32>,
}

impl Paging {
    pub fn first(limit: u32) -> Self {
        Paging {
            after: None,
            limit: Some(limit),
        }
    }

    pub fn after(cursor: Cursor, limit: u32) -> Self {
        Paging {
            after: Some(cursor),
            limit: Some(limit),
        }
    }

    /// Clamped rather than refused: a client asking for a thousand rows wants
    /// as many as it can have, and failing its request helps nobody.
    pub fn limit(&self) -> u32 {
        self.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
    }

    /// One more than asked for, so a full page tells us another exists without
    /// a second query counting the rest.
    pub(crate) fn probe(&self) -> i64 {
        i64::from(self.limit()) + 1
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<String>,
}

impl<T> Page<T> {
    /// Trims the probe row and turns it into the cursor for the next page.
    pub(crate) fn build(mut items: Vec<T>, paging: Paging, cursor: impl Fn(&T) -> Cursor) -> Self {
        let limit = paging.limit() as usize;
        let more = items.len() > limit;
        items.truncate(limit);

        let next = if more {
            items.last().map(|last| cursor(last).encode())
        } else {
            None
        };

        Page { items, next }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

impl<T> IntoIterator for Page<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor() -> Cursor {
        Cursor {
            at: chrono::DateTime::from_timestamp_micros(1_700_000_000_000_000)
                .expect("a timestamp"),
            id: Uuid::now_v7(),
        }
    }

    #[test]
    fn a_cursor_survives_the_round_trip() {
        let start = cursor();
        let back = Cursor::decode(&start.encode()).expect("its own encoding decodes");
        assert_eq!(start, back);
    }

    #[test]
    fn text_that_is_not_a_cursor_is_refused() {
        assert!(Cursor::decode("hello").is_err());
        assert!(Cursor::decode("").is_err());
    }

    #[test]
    fn a_limit_beyond_the_ceiling_is_brought_down_to_it() {
        assert_eq!(Paging::first(10_000).limit(), MAX_LIMIT);
        assert_eq!(Paging::first(0).limit(), 1);
    }

    #[test]
    fn a_full_page_offers_a_next_and_a_short_one_does_not() {
        let items: Vec<u8> = (0..4).collect();
        let page = Page::build(items, Paging::first(3), |_| cursor());
        assert_eq!(page.len(), 3);
        assert!(page.next.is_some());

        let page = Page::build(vec![1u8, 2], Paging::first(3), |_| cursor());
        assert!(page.next.is_none());
    }
}
