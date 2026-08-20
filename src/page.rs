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

/// Where the last page stopped, in whichever ordering it was issued under.
///
/// Carries a key and an id because a key alone is not unique: two rows with
/// the same timestamp — or the same title — would make the boundary
/// ambiguous, and one of them would be skipped or repeated.
///
/// The key is not always a timestamp. A list ordered by title needs a title
/// to resume from, and a cursor that only knew about time could not carry
/// one; that is the whole reason this is not a pair of fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    key: Key,
    pub id: Uuid,
}

/// What a cursor resumes from. One variant per kind of column a list can be
/// ordered by, and nothing wider: a key is bound into a query, so its type
/// has to be a type Postgres was given.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    At(chrono::DateTime<chrono::Utc>),
    Text(String),
}

impl Cursor {
    /// A cursor into a list ordered by time, which is most of them.
    pub fn at(at: chrono::DateTime<chrono::Utc>, id: Uuid) -> Self {
        Cursor {
            key: Key::At(at),
            id,
        }
    }

    /// A cursor into a list ordered by something a person reads — a title, a
    /// name, a code.
    pub fn text(value: impl Into<String>, id: Uuid) -> Self {
        Cursor {
            key: Key::Text(value.into()),
            id,
        }
    }

    /// The timestamp this resumes from, when it was issued under a time
    /// ordering. `None` means the query should not bind it — and the query
    /// carries a predicate for each kind, so the one that is `None` is the
    /// one that does nothing.
    pub fn timestamp(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        match &self.key {
            Key::At(at) => Some(*at),
            Key::Text(_) => None,
        }
    }

    /// The same, for a list ordered by text.
    pub fn text_key(&self) -> Option<&str> {
        match &self.key {
            Key::At(_) => None,
            Key::Text(value) => Some(value),
        }
    }

    /// Opaque on purpose: a client that takes one apart will depend on how the
    /// ordering works, and then the ordering cannot change.
    pub fn encode(&self) -> String {
        let raw = match &self.key {
            Key::At(at) => format!("t|{}|{}", at.timestamp_micros(), self.id),
            // The id before the value, so a value containing the separator
            // costs nothing: everything after the second `|` is the key.
            Key::Text(value) => format!("s|{}|{}", self.id, value),
        };
        URL_SAFE_NO_PAD.encode(raw)
    }

    pub fn decode(text: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD
            .decode(text)
            .map_err(|_| Error::invalid("that is not a cursor"))?;
        let raw = String::from_utf8(bytes).map_err(|_| Error::invalid("that is not a cursor"))?;

        let mut parts = raw.splitn(3, '|');
        let first = parts.next().unwrap_or_default();
        let second = parts.next();
        let third = parts.next();

        match (first, second, third) {
            ("t", Some(micros), Some(id)) => Ok(Cursor::at(timestamp(micros)?, uuid(id)?)),
            ("s", Some(id), Some(value)) => Ok(Cursor::text(value, uuid(id)?)),
            // Cursors issued before there was more than one kind of key. Kept
            // readable so a page somebody had open, or a link they sent, does
            // not answer `invalid` the day this shipped.
            (micros, Some(id), None) => Ok(Cursor::at(timestamp(micros)?, uuid(id)?)),
            _ => Err(Error::invalid("that is not a cursor")),
        }
    }
}

fn timestamp(micros: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    let micros: i64 = micros
        .parse()
        .map_err(|_| Error::invalid("that is not a cursor"))?;
    chrono::DateTime::from_timestamp_micros(micros)
        .ok_or_else(|| Error::invalid("that is not a cursor"))
}

fn uuid(text: &str) -> Result<Uuid> {
    Uuid::parse_str(text).map_err(|_| Error::invalid("that is not a cursor"))
}

/// Which end of the list comes first.
///
/// Every list in this crate has answered `Oldest` since it was written, which
/// is right for a shopper walking a catalogue and backwards for a back office:
/// an operator opening Orders wants today's, not the first order the shop ever
/// took.
///
/// This costs nothing to support and needs no new cursor, which is worth
/// saying plainly because it looks like it should. A cursor here is
/// `(created_at, id)` — the sort key of either direction — so newest-first is
/// the same tuple compared the other way and ordered the other way. Sorting
/// by some other column is the change this is not: that needs the cursor to
/// carry that column's value instead of a timestamp, and it is not done.
///
/// This doc comment reaches the OpenAPI document and from there a code
/// generator, which escapes what it finds — so it is written without markdown
/// emphasis. An asterisk here became a lint error in generated TypeScript.
///
/// It lives on a filter rather than on [`Paging`] on purpose. Four lists
/// honour it and sixty-odd do not; on `Paging` those sixty-odd would take a
/// direction and silently ignore it, which is the failure this codebase has
/// written down more than once. On a filter, a list that cannot answer never
/// offers the question.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    /// Oldest first — what every list did before there was a choice.
    #[default]
    Oldest,
    /// Newest first.
    Newest,
}

impl Order {
    /// How a row is compared against the cursor: past it, in whichever
    /// direction "past" means here.
    pub fn beyond(self) -> &'static str {
        match self {
            Order::Oldest => ">",
            Order::Newest => "<",
        }
    }

    /// What follows each column of the `order by`.
    pub fn direction(self) -> &'static str {
        match self {
            Order::Oldest => "asc",
            Order::Newest => "desc",
        }
    }
}

/// Which column a list is ordered by.
///
/// One variant per column any list here can be ordered by, rather than a
/// string a caller picks: the column name is interpolated into SQL, and a
/// closed set is what makes that safe to read as well as to run.
///
/// A list that offers a second ordering needs a cursor that can resume from
/// it — which is why [`Cursor`] carries a key rather than a timestamp. The
/// two go together: adding a variant here without teaching the list to bind
/// the matching key gives an ordering that pages back to the beginning.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum By {
    /// When the row was written. What every list did before there was a
    /// choice, and what a list with nothing better to offer still does.
    #[default]
    Created,
    /// What a person reads the row as — a product's title.
    Title,
}

/// What somebody typed into a search box.
///
/// A back office with forty thousand orders cannot find one by paging to it,
/// and until this there was nothing else to find it with: every list took a
/// cursor and a limit and nothing else. This is the narrow answer — the words
/// a person types, matched against the few columns a person would recognise
/// the row by — not a query language and not an index. What each list matches
/// is written where that list is, because "searchable" is a claim about
/// columns rather than about strings.
///
/// It is a type rather than a `String` for one reason, and it is not
/// ceremony: the pattern below has to escape what the operator typed. A shop
/// searching for `50%` off means the two characters, and `%` is the wildcard
/// that would otherwise match every row in the table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search(String);

impl Search {
    /// `None` for a blank box, and for one holding only spaces: an empty
    /// search is not a search, and treating it as one would quietly show a
    /// filtered list that filtered nothing.
    pub fn new(text: &str) -> Option<Self> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(Search(trimmed.to_owned()))
    }

    /// What goes into an `ilike`. `%` and `_` are Postgres's own wildcards and
    /// `\` is its escape, so all three are escaped before the pattern is
    /// wrapped — otherwise a search for `_` matches every row of one
    /// character, and a search for `%` matches everything there is.
    pub fn pattern(&self) -> String {
        let mut escaped = String::with_capacity(self.0.len() + 2);
        escaped.push('%');
        for character in self.0.chars() {
            if matches!(character, '%' | '_' | '\\') {
                escaped.push('\\');
            }
            escaped.push(character);
        }
        escaped.push('%');
        escaped
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a caller asks for.
///
/// `Clone` and not `Copy`: a cursor carries a key, and a key can be a title —
/// so a `Paging` owns a `String` and moving one has to be visible.
#[derive(Debug, Clone, Default)]
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
    pub fn probe(&self) -> i64 {
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
    pub fn build(mut items: Vec<T>, paging: Paging, cursor: impl Fn(&T) -> Cursor) -> Self {
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

/// Written by hand rather than derived: a derive names the schema after `T`,
/// which would give every list its own `Page_of_X` and the document 67 copies
/// of one envelope. This one names itself `Page` regardless of `T` and says
/// nothing about what `items` holds — `src/api/openapi.rs` overlays the real
/// item schema per operation with `allOf`.
impl<T> schemars::JsonSchema for Page<T> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Page".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        "tezgah::page::Page".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "properties": {
                "items": { "type": "array", "items": true },
                "next": { "type": ["string", "null"] },
            },
            "required": ["items"],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor() -> Cursor {
        Cursor::at(
            chrono::DateTime::from_timestamp_micros(1_700_000_000_000_000).expect("a timestamp"),
            Uuid::now_v7(),
        )
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

    #[test]
    fn an_order_is_the_comparison_and_the_direction_together() {
        // The two have to agree: a `<` with an `asc` walks away from the
        // cursor and pages backwards through a list that never ends.
        assert_eq!(Order::default(), Order::Oldest);
        assert_eq!(Order::Oldest.beyond(), ">");
        assert_eq!(Order::Oldest.direction(), "asc");
        assert_eq!(Order::Newest.beyond(), "<");
        assert_eq!(Order::Newest.direction(), "desc");
    }

    #[test]
    fn a_blank_box_is_not_a_search() {
        assert_eq!(Search::new(""), None);
        assert_eq!(Search::new("   "), None);
        assert_eq!(Search::new(" denim "), Search::new("denim"));
    }

    #[test]
    fn the_operators_own_wildcards_are_escaped() {
        // Without this, "50%" is "50 followed by anything", which is every
        // row that starts with 50 — and "_" is every row of one character.
        let search = Search::new("50%").expect("not blank");
        assert_eq!(search.pattern(), "%50\\%%");

        let underscore = Search::new("a_b").expect("not blank");
        assert_eq!(underscore.pattern(), "%a\\_b%");

        let backslash = Search::new("a\\b").expect("not blank");
        assert_eq!(backslash.pattern(), "%a\\\\b%");
    }

    #[test]
    fn an_ordinary_word_is_wrapped_and_nothing_else() {
        let search = Search::new("denim jacket").expect("not blank");
        assert_eq!(search.pattern(), "%denim jacket%");
    }

    #[test]
    fn a_cursor_survives_a_round_trip_whichever_key_it_carries() {
        let id = Uuid::now_v7();
        let at =
            chrono::DateTime::from_timestamp_micros(1_700_000_000_000_000).expect("a timestamp");

        let timed = Cursor::at(at, id);
        assert_eq!(Cursor::decode(&timed.encode()).expect("to decode"), timed);
        assert_eq!(timed.timestamp(), Some(at));
        assert_eq!(timed.text_key(), None);

        let titled = Cursor::text("A denim jacket", id);
        assert_eq!(Cursor::decode(&titled.encode()).expect("to decode"), titled);
        assert_eq!(titled.text_key(), Some("A denim jacket"));
        assert_eq!(titled.timestamp(), None);
    }

    #[test]
    fn a_key_may_contain_the_separator() {
        // The id goes before the value for exactly this: a product called
        // "denim | blue" is a product, and its cursor has to survive being one.
        let id = Uuid::now_v7();
        let awkward = Cursor::text("denim | blue", id);
        assert_eq!(
            Cursor::decode(&awkward.encode()).expect("to decode"),
            awkward
        );
    }

    #[test]
    fn a_cursor_from_before_there_were_two_kinds_still_reads() {
        // What `encode` used to write: micros, a separator, the id — and
        // somebody may have one in a URL they had open.
        let id = Uuid::now_v7();
        let old = URL_SAFE_NO_PAD.encode(format!("1700000000000000|{id}"));

        let read = Cursor::decode(&old).expect("an old cursor still decodes");
        assert_eq!(read.id, id);
        assert_eq!(
            read.timestamp(),
            chrono::DateTime::from_timestamp_micros(1_700_000_000_000_000)
        );
    }
}
