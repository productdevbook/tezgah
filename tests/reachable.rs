//! "Something calls it, or somebody said why nothing does."
//!
//! Five features shipped green and unreachable — gift cards and store credit,
//! lot tracking, the tax identity tables, agreements and invoices, sales
//! channels — and in every one of them the module was right, its tests passed,
//! and nothing called it. Issue #113 is that audit turned into a test.
//!
//! Three questions, none of which needs Postgres — the source and the
//! migrations answer all three, so these run outside the database test group:
//!
//! 1. every free `pub fn` outside `src/api/` has a caller somewhere else in
//!    the crate;
//! 2. every table a migration creates has a writer — in `src/`, resolved
//!    through a `format!`'s own placeholder where the table name is not a
//!    literal string there, or in a trigger a migration installs;
//! 3. every value a `check (col in (..))` permits appears as a `'literal'`
//!    somewhere in `src/`.
//!
//! What this does not prove: that a caller is on a path a request can reach,
//! or that a route is wired up by the host, or that the value written is
//! written in the right column. Names are matched textually — a caller is a
//! `module::name(` elsewhere, or a bare `name(` in a file that imported it —
//! so two functions sharing a name share their callers.
//!
//! Check 2 resolves a `format!`-built table name three ways: a private
//! function's own parameter, read from every literal argument its callers in
//! the same file pass (`table` in `catalogue::link` and `catalogue::unlink`);
//! a tuple element bound from a same-file method whose body is nothing but
//! a `match` returning literal tuples (`adjustment_table`/`tax_table`, bound
//! from `LineMoney::tables` in `order::insert_line_money`); or a tuple element
//! bound from a `for` loop over an array of literal tuples written inline
//! (`table` in `tax::set_cart_tax_lines`, writing `cart_line_item_tax_line`
//! and `cart_shipping_method_tax_line`). All three are shapes
//! `tests/format_sql.rs` already proved closed over literals, for the
//! opposite reason — that check is proving a caller cannot smuggle a value
//! in; this one is proving what the table name always resolves to. A table
//! name built any other way — a loop variable with nothing to destructure,
//! the shape `order::erase` uses to delete `order_status_history` alongside
//! others — resolves to nothing here, which costs no coverage there either:
//! the tuple-destructure case above already wrote those tables down through
//! `LineMoney::tables`. Until issue #187 this check also counted a table's
//! name in quotes anywhere in `src/` as proof of a writer, which let
//! `Error::not_found("currency")` pass for a table nothing wrote a row into;
//! that form is gone, and the
//! quoted forms it keeps are `insert into "T"`/`update "T"`/`delete from
//! "T"` — the shape a reserved word like `"order"` is genuinely written in,
//! not a name appearing anywhere for any reason. And check 2 reads
//! `migrations/` for one more shape entirely: a trigger's own body between
//! its `$$` delimiters, so `order_status_history` — written only by
//! `tezgah_order_status_moves`, installed in migration 0020 — counts, where
//! reading `src/` alone would have missed it, the same blind spot #118
//! documented for check 3 below.
//!
//! Check 2 also asks a narrower question of the same evidence: not just
//! whether a table has a writer, but whether anything can put a *first* row
//! in it — an `insert`, never an `update` or a `delete` alone. `store` was
//! the gap #190 found: `update store set ...` is a real writer by the wider
//! question, so the table read as healthy, but nothing ever inserted into
//! it, so the update always matched zero rows. The two questions share their
//! reading of `src/` and `migrations/` and differ only in which prefix they
//! count — `table_is_written` all three, `table_can_hold_a_row` `insert`
//! alone — so a table can now fail either one, with its own message: no
//! writer at all, tolerated in `TOLERATED_TABLES`, or a writer that can
//! never create the row it updates, which nothing may tolerate.
//!
//! Check 3 is weaker than checks 1 and 2, and it is worth being exact about
//! how. It does not distinguish where a literal sits — a value in a `match`
//! arm or a `where` clause passes the same as one in an `insert`, so `'lot'`
//! and `'serial'` (#110) and eight `withdrawal_exclusion` reasons (#111)
//! passed this check while nothing could write any of them; both were found
//! by hand. It reads only `src/`, so a value a trigger writes in
//! `migrations/` — `order.fulfillment_status`, moved by
//! `tezgah_order_fulfillment_status` in migration 0022 on every `order_item`
//! change — looks unwritten when it is not. And it cannot follow data: a
//! column fed by a caller's string, validated at insert time by the check
//! constraint itself, has no literal to find even though the value is fully
//! reachable — `customer_tax_id.tax_id_type` and `tax_exemption.kind` are
//! exactly this, now that #107's routes exist. None of that makes the check
//! worthless — a value with no literal and no writer anywhere is still worth
//! a human look — but its tolerated reasons say what the check actually
//! found: "not a literal in `src/`", not "unreachable". See #118.
//!
//! Constraints are read from `migrations/` rather than the catalogue, where
//! `tests/schema.rs` reads them. The catalogue is the more accurate of the
//! two, and knowing that, the migrations still win here: a check needing a
//! database would put this test in the pool-bounded group, and what it is
//! looking for — a value nobody writes — does not change between the file and
//! the catalogue. The last constraint written for a column is the one that
//! counts, so a migration narrowing an older one is honoured.
//!
//! Everything unreachable today is in a `TOLERATED` list with the reason it is
//! not a hole, the shape `tests/permit_asked.rs` uses. The lists may only
//! shrink: an entry naming an issue leaves when that issue closes, and an
//! entry claiming a function is the embedding host's to call is a claim a
//! reader can argue with.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Public functions nothing in the crate calls, each with the reason.
/// Adding to this is not a fix.
const TOLERATED: [(&str, &str); 48] = [
    (
        "batch::import_workflow",
        "the import workflow a host runs through the runner when a file is large \
         enough to want checkpoints",
    ),
    (
        "cart::set_customer",
        "the plain setter; the storefront moves a cart to its customer through \
         `cart::transfer_to_customer`",
    ),
    (
        "cart::list",
        "a page of carts, which no surface asks for: a storefront reaches its \
         own cart by id and the back office has no cart screen",
    ),
    (
        "cart::expire",
        "a sweep a host runs on a schedule; there is no request to hang it off",
    ),
    (
        "customer::by_email",
        "a lookup by e-mail with no caller and no route; a host's sign-in is the \
         only plausible caller",
    ),
    (
        "customer::group_ids",
        "the groups a customer is in, read by nobody; the pricing context is \
         built from ids the caller already has",
    ),
    (
        "fulfilment::create_geo_zone",
        "no route creates a geo zone: a zone is made through its service zone \
         today",
    ),
    (
        "fulfilment::zones_for",
        "every zone an address falls into; nothing asks, and no route offers it",
    ),
    (
        "fulfilment::priced_options_for",
        "options priced by asking the carrier; the storefront lists the shop- \
         priced ones only",
    ),
    (
        "fulfilment::create_fulfillment_with",
        "the variant taking an explicit provider rather than the shop's default; \
         no route passes one",
    ),
    (
        "fulfilment::cancel_fulfillment_with",
        "the variant taking an explicit provider rather than the shop's default; \
         no route passes one",
    ),
    (
        "inventory::expire_reservations",
        "a sweep a host runs on a schedule; there is no request to hang it off",
    ),
    (
        "inventory::reservations_for_line_item",
        "the reservations behind one line, read by nobody and reachable through \
         no route",
    ),
    (
        "inventory::availability_for_variant",
        "what could still be sold of a variant, counting its bundle; no route \
         answers the question",
    ),
    (
        "order::can_transition",
        "a pure predicate over two statuses, exported so a host can grey out a \
         button before it asks",
    ),
    (
        "order::write_summary",
        "derives a summary row from rows its caller already read; in-crate \
         callers reach it through the version write",
    ),
    (
        "order::invoice",
        "invoices and credit notes have no route and no in-crate caller; shipped \
         unwired, see #111",
    ),
    (
        "payment::register_provider",
        "a host registering the providers it has assembled, once at start-up",
    ),
    (
        "payment::provider_by_code",
        "a provider row by its code, with no caller and no route",
    ),
    (
        "payment::flag_mismatch",
        "records that a provider moved an amount nobody asked for; the host's \
         webhook handler is the only place that can know",
    ),
    (
        "payment::session",
        "one session by id; the storefront creates sessions and reads the \
         collection, never a session on its own",
    ),
    (
        "payment::balance",
        "what is still capturable and refundable; the admin payment view sums it \
         its own way",
    ),
    (
        "payment::record_webhook",
        "the host's webhook handler writes the event, acts on it and marks it; no \
         route receives a provider's callback for it",
    ),
    (
        "payment::mark_processed",
        "the host's webhook handler writes the event, acts on it and marks it; no \
         route receives a provider's callback for it",
    ),
    (
        "payment::mark_failed",
        "the host's webhook handler writes the event, acts on it and marks it; no \
         route receives a provider's callback for it",
    ),
    (
        "payment::unprocessed",
        "the host's webhook handler writes the event, acts on it and marks it; no \
         route receives a provider's callback for it",
    ),
    (
        "pricing::link_shipping_option",
        "links a shipping option to a price set; the option's own create writes \
         the link today",
    ),
    (
        "iyzico::authorization_header",
        "verifying a provider's webhook signature, which the host does before it \
         hands the body over",
    ),
    (
        "iyzico::verify_webhook_signature",
        "verifying a provider's webhook signature, which the host does before it \
         hands the body over",
    ),
    (
        "iyzico::webhook_signature",
        "verifying a provider's webhook signature, which the host does before it \
         hands the body over",
    ),
    (
        "iyzico::read_event",
        "verifying a provider's webhook signature, which the host does before it \
         hands the body over",
    ),
    (
        "stripe::read_event",
        "parses a provider's webhook body, which the host does before it hands \
         anything to the library",
    ),
    (
        "stripe::verify_signature",
        "verifying a provider's webhook signature, which the host does before it \
         hands the body over",
    ),
    (
        "stripe::signature_header",
        "verifying a provider's webhook signature, which the host does before it \
         hands the body over",
    ),
    (
        "kasapay::to_kasapay_money",
        "#53's design note stages this as its own PR — proving the Money \
         conversion round-trips before anything downstream depends on it, so \
         nothing does yet",
    ),
    (
        "kasapay::from_kasapay_money",
        "the inverse of kasapay::to_kasapay_money, same reason",
    ),
    (
        "kasapay::capture",
        "kept ready for #53's PaymentProvider wrapper; that step waits on \
         kasapay#149 (Currency is nine variants, Stripe settles in 135+) \
         before src/providers/stripe.rs can go",
    ),
    ("kasapay::cancel", "same as kasapay::capture"),
    ("kasapay::refund", "same as kasapay::capture"),
    ("kasapay::lookup", "same as kasapay::capture"),
    (
        "tax::rates_for",
        "the rates answering for an address, used by nobody outside its own \
         module",
    ),
    (
        "tax::calculate_with",
        "tax worked out by somebody else's engine, for a shop whose tax is not \
         tezgah's; a host wires its provider in",
    ),
    (
        "tax::refund_with",
        "tax worked out by somebody else's engine, for a shop whose tax is not \
         tezgah's; a host wires its provider in",
    ),
    (
        "workflow::step",
        "boxes a step for `Workflow::parallel`; a host composing its own workflow \
         calls it",
    ),
    (
        "workflow::start",
        "a host starting a run it will drive itself with `work`",
    ),
    (
        "workflow::recover",
        "the runner putting back what its dead workers held; a host's supervisor \
         calls it, no route can",
    ),
    (
        "workflow::extend",
        "a running step extending its own lease, called by whatever is executing \
         it",
    ),
    (
        "workflow::work",
        "the host's own runner: it drives runs rather than being driven by one",
    ),
];

/// Tables no code in `src/` writes to, each with the reason.
const TOLERATED_TABLES: [(&str, &str); 7] = [
    (
        "tezgah_table",
        "the register a migration writes through `tezgah_register`, read by \
         the schema tests; SQL owns it, not the library",
    ),
    (
        "tezgah_scope",
        "the scope column's own bookkeeping, written by the migration that \
         adds a scope",
    ),
    (
        "tezgah_scoped_fk_table",
        "which tables carry a scoped foreign key, written by the migrations \
         that scoped them",
    ),
    (
        "tezgah_cross_scope_fk",
        "which single-column keys are named, deliberate exceptions to same-scope \
         keys, written by `tezgah_cross_scope_fk` the procedure, not the library",
    ),
    (
        "tezgah_evidence_table",
        "which tables hold evidence a change may not edit away, declared in \
         SQL beside the trigger that enforces it",
    ),
    (
        "tezgah_order_status_move",
        "the allowed status moves, declared in SQL so the database refuses \
         one the library never asked for",
    ),
    (
        "tezgah_product_status_move",
        "the same shape for a product's status, since `proposed` and \
         `rejected` need the same database-level backstop",
    ),
];

/// Values a check constraint permits and no `'literal'` for appears in
/// `src/`. That is all this check can see — not that the value is
/// unreachable. Each reason says which of the three shapes it is: written by
/// SQL the scanner does not read, written from a caller's string the
/// scanner cannot follow, or genuinely not offered yet, in which case the
/// reason names the issue.
const TOLERATED_VALUES: [(&str, &str); 16] = [
    (
        "order.fulfillment_status = 'not_fulfilled'",
        "written by the tezgah_order_fulfillment_status trigger installed in \
         migration 0022 on every order_item change, not by a literal in \
         src/; this check only reads src/, see #118",
    ),
    (
        "order.fulfillment_status = 'partially_fulfilled'",
        "same trigger, same reason: written in SQL, not as a literal in src/",
    ),
    (
        "order.fulfillment_status = 'partially_shipped'",
        "same trigger, same reason: written in SQL, not as a literal in src/",
    ),
    (
        "order.fulfillment_status = 'partially_delivered'",
        "same trigger, same reason: written in SQL, not as a literal in src/",
    ),
    (
        "order.fulfillment_status = 'partially_returned'",
        "same trigger, same reason: written in SQL, not as a literal in src/",
    ),
    (
        "order.fulfillment_status = 'returned'",
        "same trigger, same reason: written in SQL, not as a literal in src/",
    ),
    (
        "order_return.status = 'open'",
        "a return is written `requested` and moves on from there; 'open' is a \
         state the library never puts a return in — a deliberate omission, \
         not missing work",
    ),
    (
        "customer_tax_id.tax_id_type = 'ein'",
        "reachable now: #107's routes take this as a caller-supplied string, \
         validated at insert by this same check constraint; the value has no \
         literal in src/ for this check to find, see #118",
    ),
    (
        "customer_tax_id.tax_id_type = 'vkn'",
        "same column, same reason: a caller's string, not a literal in src/",
    ),
    (
        "customer_tax_id.tax_id_type = 'tckn'",
        "same column, same reason: a caller's string, not a literal in src/",
    ),
    (
        "customer_tax_id.tax_id_type = 'gst'",
        "same column, same reason: a caller's string, not a literal in src/",
    ),
    (
        "customer_tax_id.tax_id_type = 'abn'",
        "same column, same reason: a caller's string, not a literal in src/",
    ),
    (
        "tax_exemption.kind = 'nonprofit'",
        "reachable now: #107's routes take this as a caller-supplied string, \
         validated at insert by this same check constraint; the value has no \
         literal in src/ for this check to find, see #118",
    ),
    (
        "tax_exemption.kind = 'government'",
        "same column, same reason: a caller's string, not a literal in src/",
    ),
    (
        "tax_exemption.kind = 'diplomatic'",
        "same column, same reason: a caller's string, not a literal in src/",
    ),
    (
        "tax_exemption.kind = 'export'",
        "same column, same reason: a caller's string, not a literal in src/",
    ),
];

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn files(dir: &Path, extension: &str, into: &mut Vec<(PathBuf, String)>) {
    for entry in std::fs::read_dir(dir).expect("the directory to be readable") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            files(&path, extension, into);
        } else if path.extension().is_some_and(|e| e == extension) {
            let text = std::fs::read_to_string(&path).expect("a readable file");
            into.push((path, text));
        }
    }
}

fn sources() -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    files(&root().join("src"), "rs", &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!out.is_empty(), "no source files were scanned");
    out
}

/// The module a file is: `src/tax.rs` is `tax`, `src/providers/mod.rs` is
/// `providers`, which is how the rest of the crate names them.
fn module(path: &Path) -> String {
    let stem = path
        .file_stem()
        .expect("a file name")
        .to_string_lossy()
        .to_string();
    if stem == "mod" {
        path.parent()
            .and_then(|p| p.file_name())
            .expect("a parent directory")
            .to_string_lossy()
            .to_string()
    } else {
        stem
    }
}

/// Every `pub fn` written at the left margin: a free function, not a method on
/// an `impl`, which is what a caller reaches as `module::name(..)`.
fn free_public_functions(source: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let Some(rest) = line.strip_prefix("pub") else {
            continue;
        };
        let rest = match rest.strip_prefix('(') {
            Some(visibility) => match visibility.find(')') {
                Some(end) => &visibility[end + 1..],
                None => continue,
            },
            None => rest,
        };
        let Some(rest) = rest.strip_prefix(' ') else {
            continue;
        };
        let rest = rest.strip_prefix("async ").unwrap_or(rest);
        let Some(rest) = rest.strip_prefix("fn ") else {
            continue;
        };
        let name: String = rest.chars().take_while(|c| ident_char(*c)).collect();
        if !name.is_empty() {
            out.push((name, index + 1));
        }
    }
    out
}

/// Whether `haystack` calls `name`: qualified as `module::name(`, or bare
/// where the file imported the name.
fn calls(haystack: &str, module: &str, name: &str) -> bool {
    let qualified = format!("{module}::{name}");
    let mut at = 0;
    while let Some(found) = haystack[at..].find(&qualified) {
        let start = at + found;
        let after = start + qualified.len();
        let opens = haystack[after..].trim_start().starts_with('(');
        if opens && !haystack[..start].ends_with(ident_char) {
            return true;
        }
        at = after;
    }

    let imported = haystack
        .split(';')
        .filter(|statement| statement.trim_start().starts_with("use "))
        .any(|statement| {
            statement
                .split(|c: char| !ident_char(c))
                .any(|word| word == name)
        });
    if !imported {
        return false;
    }

    let chars: Vec<char> = haystack.chars().collect();
    let wanted: Vec<char> = name.chars().collect();
    for start in 0..chars.len() {
        if !chars[start..].starts_with(&wanted[..]) {
            continue;
        }
        let before = start.checked_sub(1).map(|i| chars[i]);
        if before.is_some_and(|c| ident_char(c) || c == ':' || c == '.') {
            continue;
        }
        let mut after = start + wanted.len();
        while after < chars.len() && chars[after] == ' ' {
            after += 1;
        }
        if chars.get(after) == Some(&'(') {
            return true;
        }
    }
    false
}

#[test]
fn every_public_function_has_a_caller() {
    let sources = sources();

    let mut unreachable = Vec::new();
    let mut used = BTreeSet::new();

    for (path, source) in &sources {
        if path.components().any(|c| c.as_os_str() == "api") {
            continue;
        }
        let module = module(path);
        for (name, line) in free_public_functions(source) {
            let called = sources
                .iter()
                .any(|(other, text)| other != path && calls(text, &module, &name));
            if called {
                continue;
            }
            let at = format!("{module}::{name}");
            match TOLERATED.iter().find(|(known, _)| *known == at) {
                Some((known, _)) => {
                    used.insert(*known);
                }
                None => unreachable.push(format!(
                    "{}:{line} — {at}",
                    path.strip_prefix(root()).unwrap_or(path).display()
                )),
            }
        }
    }

    assert!(
        unreachable.is_empty(),
        "nothing in the crate calls these, and no route reaches them:\n  {}\n\
         Wire them up, or name them in TOLERATED with the reason a host is \
         expected to call them itself.",
        unreachable.join("\n  ")
    );

    let stale: Vec<&str> = TOLERATED
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !used.contains(name))
        .collect();
    assert!(
        stale.is_empty(),
        "these have a caller now, or are gone; take them out of TOLERATED: {stale:?}"
    );
}

/// Handlers with no route, or routes with no handler: the second half of the
/// same question, asked where `src/api/` answers it.
#[test]
fn every_route_handler_is_declared_in_routes() {
    const SURPLUS: [(&str, usize, &str); 2] = [
        (
            "store",
            1,
            "`reprice` has no route of its own: every cart mutation here \
             reprices, and it is public so a host that moved a line another \
             way can too",
        ),
        (
            "openapi",
            1,
            "`document` builds the OpenAPI paper from ROUTES; a host serves \
             it, and it is not itself one of them",
        ),
    ];

    let mut sources = Vec::new();
    files(&root().join("src").join("api"), "rs", &mut sources);
    sources.sort_by(|a, b| a.0.cmp(&b.0));

    let mut wrong = Vec::new();
    for (path, source) in &sources {
        let module = module(path);
        if module == "mod" || module == "api" {
            continue;
        }
        let handlers = free_public_functions(source).len();
        let declared = match source.find("static ROUTES") {
            Some(at) => {
                source[at..].matches("Route {").count() + source[at..].matches("route!(").count()
            }
            None => 0,
        };
        let surplus = SURPLUS
            .iter()
            .find(|(name, _, _)| *name == module)
            .map(|(_, count, _)| *count)
            .unwrap_or(0);
        if handlers != declared + surplus {
            wrong.push(format!(
                "src/api/{module}.rs — {handlers} public functions, {declared} routes\
                 {}",
                if surplus > 0 {
                    format!(" and {surplus} tolerated without one")
                } else {
                    String::new()
                }
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "a handler with no route is unreachable and a route with no handler is \
         a 404:\n  {}",
        wrong.join("\n  ")
    );
}

fn migrations() -> String {
    let mut out = Vec::new();
    files(&root().join("migrations"), "sql", &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!out.is_empty(), "no migrations were scanned");
    out.into_iter()
        .map(|(_, text)| text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The word after `token` in `text`, wherever it appears: `create table x`
/// gives `x`.
fn words_after(text: &str, token: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = text[at..].find(token) {
        let start = at + found + token.len();
        let rest = text[start..].trim_start_matches([' ', '\n', '\t']);
        let rest = rest.strip_prefix("if not exists ").unwrap_or(rest);
        let rest = rest.strip_prefix("if exists ").unwrap_or(rest);
        let rest = rest.strip_prefix('"').unwrap_or(rest);
        let name: String = rest.chars().take_while(|c| ident_char(*c)).collect();
        if !name.is_empty() {
            out.push(name);
        }
        at = start;
    }
    out
}

/// The index one past the `)` matching the `(` at `open`, skipping the
/// contents of `"..."` string literals. `None` if it never closes. This and
/// the handful of helpers below it duplicate `tests/format_sql.rs`'s own —
/// each latch here reads the crate on its own, the same duplication
/// `sources`/`files` above already carries.
fn matching_close_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open;
    let mut in_str = false;
    let mut escape = false;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// Same as [`matching_close_paren`], for `[` and `]`.
fn matching_close_bracket(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open;
    let mut in_str = false;
    let mut escape = false;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// The first `"..."` string literal at the start of `chars` (leading
/// whitespace allowed), unescaped quoting aside — its raw text between the
/// quotes.
fn leading_string_literal(chars: &[char]) -> Option<String> {
    let mut i = 0;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if chars.get(i) != Some(&'"') {
        return None;
    }
    i += 1;
    let start = i;
    let mut escape = false;
    while i < chars.len() {
        let c = chars[i];
        if escape {
            escape = false;
        } else if c == '\\' {
            escape = true;
        } else if c == '"' {
            return Some(chars[start..i].iter().collect());
        }
        i += 1;
    }
    None
}

/// Split `s` on its top-level commas — depth tracked over `(`, `[`, `<`,
/// `{`, string contents skipped.
fn split_top_level(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    let mut start = 0usize;
    let mut out = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' | '[' | '<' | '{' => depth += 1,
            ')' | ']' | '>' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(
                    chars[start..i]
                        .iter()
                        .collect::<String>()
                        .trim()
                        .to_string(),
                );
                start = i + 1;
            }
            _ => {}
        }
    }
    let last: String = chars[start..].iter().collect::<String>().trim().to_string();
    if !last.is_empty() {
        out.push(last);
    }
    out
}

fn is_string_literal(arg: &str) -> bool {
    let arg = arg.trim();
    (arg.starts_with('"') && arg.ends_with('"') && arg.len() >= 2)
        || (arg.starts_with("r\"") && arg.ends_with('"') && arg.len() >= 3)
        || (arg.starts_with("r#") && arg.ends_with('#'))
}

/// A literal's text with its surrounding quotes removed — `"table"` becomes
/// `table`.
fn strip_quotes(s: &str) -> String {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
        .to_string()
}

/// `true` if `chars[at..]` starts with the word `word` at a word boundary —
/// not preceded and not followed by an identifier character.
fn word_at(chars: &[char], at: usize, word: &[char]) -> bool {
    if at + word.len() > chars.len() {
        return false;
    }
    if at > 0 && ident_char(chars[at - 1]) {
        return false;
    }
    if &chars[at..at + word.len()] != word {
        return false;
    }
    !chars.get(at + word.len()).is_some_and(|c| ident_char(*c))
}

/// Call sites of `fn_name(` in `source`, its own `fn` declaration excluded,
/// as the raw text of the call's argument list.
fn call_sites(source: &str, fn_name: &str) -> Vec<String> {
    let chars: Vec<char> = source.chars().collect();
    let name_chars: Vec<char> = fn_name.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !word_at(&chars, i, &name_chars) {
            i += 1;
            continue;
        }
        let after = i + name_chars.len();
        let mut open = after;
        while open < chars.len() && chars[open].is_whitespace() {
            open += 1;
        }
        if chars.get(open) != Some(&'(') {
            i = after;
            continue;
        }
        // Skip the definition itself: `fn NAME(`, whitespace already spanned.
        let mut before = i;
        while before > 0 && chars[before - 1].is_whitespace() {
            before -= 1;
        }
        let is_fn_keyword = before >= 2
            && chars[before - 2] == 'f'
            && chars[before - 1] == 'n'
            && !(before >= 3 && ident_char(chars[before - 3]));
        if is_fn_keyword {
            i = open + 1;
            continue;
        }
        if let Some(close) = matching_close_paren(&chars, open) {
            out.push(chars[open + 1..close].iter().collect::<String>());
            i = close + 1;
        } else {
            i = open + 1;
        }
    }
    out
}

/// A `fn`/`async fn` found anywhere in a file — a free function or a method,
/// `impl` blocks included, since a `format!`-built table name can be bound
/// inside either.
struct SrcFunction {
    name: String,
    params: String,
    body: String,
}

/// Every function in `source`, its parameter list and body kept apart so a
/// parameter's name can be checked against what the body does with it — the
/// same walk `tests/format_sql.rs`'s `function_signatures` does.
fn src_functions(source: &str) -> Vec<SrcFunction> {
    let chars: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;

    while at + 3 < chars.len() {
        if !(chars[at] == 'f'
            && chars[at + 1] == 'n'
            && chars[at + 2].is_whitespace()
            && (at == 0 || !ident_char(chars[at - 1])))
        {
            at += 1;
            continue;
        }

        let mut name_at = at + 2;
        while name_at < chars.len() && chars[name_at].is_whitespace() {
            name_at += 1;
        }
        let name: String = chars[name_at..]
            .iter()
            .take_while(|c| ident_char(**c))
            .collect();
        if name.is_empty() {
            at += 1;
            continue;
        }

        let mut p = name_at + name.chars().count();
        while p < chars.len() && chars[p] != '(' && chars[p] != '{' && chars[p] != ';' {
            p += 1;
        }
        let params_end = if chars.get(p) == Some(&'(') {
            matching_close_paren(&chars, p)
        } else {
            None
        };
        let Some(params_end) = params_end else {
            at = name_at;
            continue;
        };
        let params: String = chars[p + 1..params_end].iter().collect();

        let mut depth = 0i32;
        let mut i = params_end + 1;
        let mut body_start = None;
        while i < chars.len() {
            match chars[i] {
                '<' | '[' => depth += 1,
                '>' | ']' => depth -= 1,
                '{' if depth <= 0 => {
                    body_start = Some(i);
                    break;
                }
                ';' if depth <= 0 => break,
                _ => {}
            }
            i += 1;
        }
        let Some(body_start) = body_start else {
            at = params_end + 1;
            continue;
        };

        let mut depth = 0i32;
        let mut j = body_start;
        while j < chars.len() {
            match chars[j] {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }

        found.push(SrcFunction {
            name,
            params,
            body: chars[body_start..j.min(chars.len())].iter().collect(),
        });

        at = body_start;
    }

    found
}

/// Every `insert into`/`update`/`delete from` a table's writer is found
/// through. [`table_is_written`] reads for `prefixes` in full — a table has
/// *a* writer if any of the three touches it. [`table_can_hold_a_row`] reads
/// for `INSERT_ONLY` — the narrower question of whether anything can ever put
/// a row in it, which `update`/`delete` alone can never answer.
const WRITE_PREFIXES: [&str; 3] = ["insert into ", "update ", "delete from "];
const INSERT_ONLY: [&str; 1] = ["insert into "];

/// The `{name}` immediately after one of `prefixes` in a `format!`'s leading
/// SQL literal — the table position, not a placeholder occurring anywhere
/// else in the string.
fn dynamic_table_placeholder(literal: &str, prefixes: &[&str]) -> Option<String> {
    let trimmed = literal.trim_start();
    for prefix in prefixes {
        let Some(rest) = trimmed.strip_prefix(prefix) else {
            continue;
        };
        let inner = rest.trim_start().strip_prefix('{')?;
        let name: String = inner.chars().take_while(|c| ident_char(*c)).collect();
        return if !name.is_empty() && inner[name.len()..].starts_with('}') {
            Some(name)
        } else {
            None
        };
    }
    None
}

/// Every dynamic table placeholder [`dynamic_table_placeholder`] finds
/// inside a `format!(..)` call anywhere in `body`, for `prefixes`.
fn dynamic_table_names_in_format_calls(body: &str, prefixes: &[&str]) -> Vec<String> {
    let chars: Vec<char> = body.chars().collect();
    let marker: Vec<char> = "format!(".chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + marker.len() <= chars.len() {
        if chars[i..i + marker.len()] != marker[..] {
            i += 1;
            continue;
        }
        let open = i + marker.len() - 1;
        if let Some(close) = matching_close_paren(&chars, open) {
            let inner = &chars[open + 1..close];
            if let Some(literal) = leading_string_literal(inner)
                && let Some(name) = dynamic_table_placeholder(&literal, prefixes)
            {
                out.push(name);
            }
        }
        i += marker.len();
    }
    out
}

/// Where `name` is bound to a tuple position by a `let (a, b, ..) =
/// expr.method(..);` in `body` — the position, and the method's name, so its
/// own literal arms can be read next.
fn tuple_binding(body: &str, name: &str) -> Option<(usize, String)> {
    let mut at = 0;
    while let Some(found) = body[at..].find("let (") {
        let start = at + found + "let (".len();
        let close = start + body[start..].find(')')?;
        let names: Vec<&str> = body[start..close].split(',').map(str::trim).collect();
        let after = close + 1;
        if let Some(position) = names.iter().position(|n| *n == name) {
            let rest = &body[after..];
            let stmt = &rest[..rest.find(';').unwrap_or(rest.len())];
            if let Some(eq) = stmt.find('=') {
                let expr = stmt[eq + 1..].trim();
                if let Some(paren) = expr.find('(')
                    && let Some(dot) = expr[..paren].rfind('.')
                {
                    let method = expr[dot + 1..paren].trim().to_string();
                    if !method.is_empty() {
                        return Some((position, method));
                    }
                }
            }
        }
        at = after;
    }
    None
}

/// Every arm of a `match` in `body` that returns a tuple literal, read at
/// `position` — `None` unless every arm found is a string literal there, so
/// a match this cannot fully read resolves to nothing rather than a partial
/// answer.
fn literal_tuple_arms(body: &str, position: usize) -> Option<Vec<String>> {
    let chars: Vec<char> = body.chars().collect();
    let marker: Vec<char> = "=> (".chars().collect();
    let mut arms = Vec::new();
    let mut i = 0;
    while i + marker.len() <= chars.len() {
        if chars[i..i + marker.len()] != marker[..] {
            i += 1;
            continue;
        }
        let open = i + marker.len() - 1;
        let close = matching_close_paren(&chars, open)?;
        let inner: String = chars[open + 1..close].iter().collect();
        let value = split_top_level(&inner).into_iter().nth(position)?;
        if !is_string_literal(&value) {
            return None;
        }
        arms.push(strip_quotes(&value));
        i = close + 1;
    }
    if arms.is_empty() { None } else { Some(arms) }
}

/// Where `name` is bound to a position by a `for (a, b, ..) in [ (..), (..)
/// ]` in `body` — every arm's literal value there, the same answer
/// [`literal_tuple_arms`] gives a `match`, for an array of tuples written
/// inline instead of returned from a method — `table`, in
/// `tax::set_cart_tax_lines`'s own loop, resolving to
/// `cart_line_item_tax_line` and `cart_shipping_method_tax_line`. `None`
/// unless every element found is a string literal there, the same
/// closed-world rule `literal_tuple_arms` holds to.
fn for_loop_tuple_arms(body: &str, name: &str) -> Option<Vec<String>> {
    let mut at = 0;
    while let Some(found) = body[at..].find("for (") {
        let start = at + found + "for (".len();
        let close = start + body[start..].find(')')?;
        let names: Vec<&str> = body[start..close].split(',').map(str::trim).collect();
        let after = close + 1;
        let Some(position) = names.iter().position(|n| *n == name) else {
            at = after;
            continue;
        };

        let rest = body[after..].trim_start();
        let Some(rest) = rest.strip_prefix("in") else {
            at = after;
            continue;
        };
        let rest = rest.trim_start();
        if !rest.starts_with('[') {
            at = after;
            continue;
        }

        let chars: Vec<char> = rest.chars().collect();
        let Some(close_bracket) = matching_close_bracket(&chars, 0) else {
            at = after;
            continue;
        };
        let inner: String = chars[1..close_bracket].iter().collect();

        let mut values = Vec::new();
        let mut ok = true;
        for element in split_top_level(&inner) {
            let element = element.trim();
            let Some(unwrapped) = element.strip_prefix('(').and_then(|e| e.strip_suffix(')'))
            else {
                ok = false;
                break;
            };
            let Some(value) = split_top_level(unwrapped).into_iter().nth(position) else {
                ok = false;
                break;
            };
            if !is_string_literal(&value) {
                ok = false;
                break;
            }
            values.push(strip_quotes(&value));
        }
        if ok && !values.is_empty() {
            return Some(values);
        }
        at = after;
    }
    None
}

/// The literal table names `name` can be at runtime, inside `function`'s own
/// body — resolved three ways: `function`'s own parameter, read from every
/// literal argument its callers in `source` pass (`table` in
/// `catalogue::link`/`catalogue::unlink`); a tuple element bound from a
/// same-file method whose body is nothing but a `match` returning literal
/// tuples (`LineMoney::tables`, bound in `order::insert_line_money`); or a
/// tuple element bound from a `for` loop over an array of literal tuples
/// written inline (`table` in `tax::set_cart_tax_lines`). See the module doc for
/// what this does and does not cover.
fn resolve_table_placeholder(
    source: &str,
    functions: &[SrcFunction],
    function: &SrcFunction,
    name: &str,
) -> Vec<String> {
    let params = split_top_level(&function.params);
    if let Some(idx) = params.iter().position(|p| {
        p.split_once(':')
            .is_some_and(|(n, _)| n.trim().trim_start_matches("mut ").trim() == name)
    }) {
        return call_sites(source, &function.name)
            .into_iter()
            .filter_map(|call| split_top_level(&call).into_iter().nth(idx))
            .filter(|arg| is_string_literal(arg))
            .map(|arg| strip_quotes(&arg))
            .collect();
    }

    if let Some((position, method)) = tuple_binding(&function.body, name)
        && let Some(target) = functions.iter().find(|f| f.name == method)
        && let Some(values) = literal_tuple_arms(&target.body, position)
    {
        return values;
    }

    if let Some(values) = for_loop_tuple_arms(&function.body, name) {
        return values;
    }

    Vec::new()
}

/// Every table a `format!`-built statement matching one of `prefixes`
/// writes, resolved through [`resolve_table_placeholder`] rather than
/// searched for as a literal string — the gap issue #187 named:
/// `tests/format_sql.rs` already proved these table names are always
/// literals its callers pass, but a table name inside a `format!` is never a
/// literal *string* in `src/` for a plain search to find.
fn tables_written_via_dynamic_sql(
    sources: &[(PathBuf, String)],
    prefixes: &[&str],
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (_, source) in sources {
        let functions = src_functions(source);
        for function in &functions {
            for name in dynamic_table_names_in_format_calls(&function.body, prefixes) {
                out.extend(resolve_table_placeholder(
                    source, &functions, function, &name,
                ));
            }
        }
    }
    out
}

/// The text between each pair of `$$` delimiters following `returns
/// trigger` in `migrations` — a trigger's own body, the one place a writer
/// can live where `src/` never sees it.
fn trigger_bodies(migrations: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(found) = migrations[at..].find("returns trigger") {
        let start = at + found;
        let Some(open_rel) = migrations[start..].find("$$") else {
            break;
        };
        let body_start = start + open_rel + 2;
        let Some(close_rel) = migrations[body_start..].find("$$") else {
            break;
        };
        let body_end = body_start + close_rel;
        out.push(migrations[body_start..body_end].to_string());
        at = body_end + 2;
    }
    out
}

/// Every table a trigger's own body writes with one of `tokens` —
/// `order_status_history`, moved by `tezgah_order_status_moves` in migration
/// 0020, is exactly this: a writer that lives in SQL and was invisible to a
/// check reading only `src/`.
fn tables_written_by_triggers(migrations: &str, tokens: &[&str]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for body in trigger_bodies(migrations) {
        for token in tokens {
            out.extend(words_after(&body, token));
        }
    }
    out
}

/// Whether `table` has a writer: a literal `insert into`/`update`/`delete
/// from` in `source`, quoted or not — `"T"` only immediately after the
/// keyword, the shape a reserved word like `"order"` needs, never a table's
/// name in quotes for any other reason — or `table` resolved through a
/// `format!` or a trigger body beforehand.
fn table_is_written(
    table: &str,
    source: &str,
    dynamic: &BTreeSet<String>,
    triggered: &BTreeSet<String>,
) -> bool {
    [
        format!("insert into {table}"),
        format!("insert into \"{table}\""),
        format!("update {table}"),
        format!("update \"{table}\""),
        format!("delete from {table}"),
        format!("delete from \"{table}\""),
    ]
    .iter()
    .any(|form| source.contains(form.as_str()))
        || dynamic.contains(table)
        || triggered.contains(table)
}

/// Whether `table` can ever hold a row: a literal `insert into`, quoted or
/// not, or `table` resolved through a `format!` or a trigger body that is
/// itself an insert. `update`/`delete` do not count here — a table only ever
/// reached by those has a writer by [`table_is_written`]'s reading and still
/// cannot hold a row, which is the gap #190 named: `store` had `update store
/// set ...` and nothing else, so the update matched no row and answered `Ok`
/// while the table stayed empty forever.
fn table_can_hold_a_row(
    table: &str,
    source: &str,
    inserted_dynamically: &BTreeSet<String>,
    inserted_by_triggers: &BTreeSet<String>,
) -> bool {
    [
        format!("insert into {table}"),
        format!("insert into \"{table}\""),
    ]
    .iter()
    .any(|form| source.contains(form.as_str()))
        || inserted_dynamically.contains(table)
        || inserted_by_triggers.contains(table)
}

#[test]
fn every_table_has_a_writer() {
    let migrations = migrations();
    let sources_list = sources();
    let source: String = sources_list
        .iter()
        .map(|(_, text)| text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    let tables: BTreeSet<String> = words_after(&migrations, "create table ")
        .into_iter()
        .collect();
    assert!(
        tables.len() > 50,
        "the migrations parsed to {} tables, which is not this schema",
        tables.len()
    );

    let dynamic = tables_written_via_dynamic_sql(&sources_list, &WRITE_PREFIXES);
    let triggered = tables_written_by_triggers(&migrations, &WRITE_PREFIXES);
    let inserted_dynamically = tables_written_via_dynamic_sql(&sources_list, &INSERT_ONLY);
    let inserted_by_triggers = tables_written_by_triggers(&migrations, &INSERT_ONLY);

    let mut unwritten = Vec::new();
    let mut update_only = Vec::new();
    let mut used = BTreeSet::new();

    for table in &tables {
        if table_can_hold_a_row(table, &source, &inserted_dynamically, &inserted_by_triggers) {
            continue;
        }
        if table_is_written(table, &source, &dynamic, &triggered) {
            update_only.push(table.clone());
            continue;
        }
        match TOLERATED_TABLES.iter().find(|(known, _)| known == table) {
            Some((known, _)) => {
                used.insert(*known);
            }
            None => unwritten.push(table.clone()),
        }
    }

    assert!(
        unwritten.is_empty(),
        "these tables exist and no code writes a row into them:\n  {}\n\
         Write to them, drop them, or name them in TOLERATED_TABLES with the \
         reason.",
        unwritten.join("\n  ")
    );

    assert!(
        update_only.is_empty(),
        "these tables are only ever updated or deleted from, never inserted \
         into, so they can never hold a row even though they have a \
         writer:\n  {}\n\
         Give them a creator — #190 was `store`, updated but never created.",
        update_only.join("\n  ")
    );

    let stale: Vec<&str> = TOLERATED_TABLES
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !used.contains(name))
        .collect();
    assert!(
        stale.is_empty(),
        "these have a writer now, or are gone; take them out of \
         TOLERATED_TABLES: {stale:?}"
    );
}

#[cfg(test)]
mod table_writer_fixtures {
    use super::*;

    #[test]
    fn a_table_name_in_an_error_message_is_not_a_writer() {
        let source = r#"Err(Error::not_found("ghost_table"))"#;
        assert!(
            !table_is_written("ghost_table", source, &BTreeSet::new(), &BTreeSet::new()),
            "a table's name in quotes anywhere in src/ must not count as a \
             writer — that is exactly issue #187"
        );
    }

    #[test]
    fn a_table_with_no_writer_at_all_is_not_written() {
        let source = "// nothing here writes to anything";
        assert!(
            !table_is_written("orphan_table", source, &BTreeSet::new(), &BTreeSet::new()),
            "a table with no insert/update/delete anywhere — literal, \
             dynamic, or triggered — must not pass"
        );
    }

    /// #190: `store` read as healthy under the old, single-question check —
    /// `update store set ...` is genuine evidence of *a* writer, and nothing
    /// else asked whether that writer could ever put a row there. A table
    /// with only `update` (or only `delete`) has to fail the narrower
    /// question even while it passes the wider one.
    #[test]
    fn a_table_written_only_by_update_cannot_hold_a_row() {
        let source = "update store set name = $1 where scope = $2";
        assert!(
            table_is_written("store", source, &BTreeSet::new(), &BTreeSet::new()),
            "an update is still a writer, by the wider question"
        );
        assert!(
            !table_can_hold_a_row("store", source, &BTreeSet::new(), &BTreeSet::new()),
            "an update alone can never put the first row in a table that has \
             none, so the narrower question has to say no"
        );
    }

    #[test]
    fn a_reserved_word_table_is_found_quoted() {
        let source = r#"sqlx::query("insert into "order" (id) values ($1)")"#;
        assert!(
            table_is_written("order", source, &BTreeSet::new(), &BTreeSet::new()),
            "a reserved word has to be quoted in valid SQL; that is still \
             genuine evidence of a write, unlike a name in quotes anywhere"
        );
    }

    #[test]
    fn a_format_built_insert_is_resolved_through_its_only_caller() {
        let source = r#"
            async fn write_link(tx: &mut Tx<'_>, table: &'static str, other: Uuid) -> Result<()> {
                sqlx::query(&format!("insert into {table} (id, other) values ($1, $2)"))
                    .bind(Uuid::now_v7())
                    .bind(other)
                    .execute(tx)
                    .await?;
                Ok(())
            }

            async fn caller(tx: &mut Tx<'_>, other: Uuid) -> Result<()> {
                write_link(tx, "widget_link", other).await
            }
        "#;
        let sources = vec![(PathBuf::from("fixture.rs"), source.to_string())];
        let dynamic = tables_written_via_dynamic_sql(&sources, &WRITE_PREFIXES);
        assert!(
            dynamic.contains("widget_link"),
            "a format!-built `insert into {{table}}` must resolve through the \
             literal its only caller passes: {dynamic:?}"
        );
    }

    #[test]
    fn a_format_built_insert_is_resolved_through_a_literal_tuple_match() {
        let source = r#"
            enum Kind {
                Line(Uuid),
                Shipping(Uuid),
            }

            impl Kind {
                fn tables(self) -> (&'static str, &'static str) {
                    match self {
                        Kind::Line(_) => ("line_adjustment", "line_tax"),
                        Kind::Shipping(_) => ("shipping_adjustment", "shipping_tax"),
                    }
                }
            }

            async fn insert_money(tx: &mut Tx<'_>, owner: Kind) -> Result<()> {
                let (adjustment_table, tax_table) = owner.tables();
                sqlx::query(&format!("insert into {adjustment_table} (id) values ($1)"))
                    .execute(tx)
                    .await?;
                sqlx::query(&format!("insert into {tax_table} (id) values ($1)"))
                    .execute(tx)
                    .await?;
                Ok(())
            }
        "#;
        let sources = vec![(PathBuf::from("fixture.rs"), source.to_string())];
        let dynamic = tables_written_via_dynamic_sql(&sources, &WRITE_PREFIXES);
        assert!(
            dynamic.contains("line_adjustment")
                && dynamic.contains("shipping_adjustment")
                && dynamic.contains("line_tax")
                && dynamic.contains("shipping_tax"),
            "a tuple element bound from a same-file method's literal match \
             arms must resolve to every arm's value: {dynamic:?}"
        );
    }

    #[test]
    fn a_format_built_insert_is_resolved_through_a_for_loop_over_literal_tuples() {
        let source = r#"
            async fn write_lines(tx: &mut Tx<'_>, items: &[Line], shipping: &[Line]) -> Result<()> {
                for (rows, table, column, parent) in [
                    (items, "widget_line_tax", "widget_line_id", "widget_line"),
                    (shipping, "widget_ship_tax", "widget_ship_id", "widget_ship"),
                ] {
                    for row in rows {
                        sqlx::query(&format!(
                            "insert into {table} (id, {column}) select $1, p.id from {parent} p"
                        ))
                        .execute(tx)
                        .await?;
                    }
                }
                Ok(())
            }
        "#;
        let sources = vec![(PathBuf::from("fixture.rs"), source.to_string())];
        let dynamic = tables_written_via_dynamic_sql(&sources, &WRITE_PREFIXES);
        assert!(
            dynamic.contains("widget_line_tax") && dynamic.contains("widget_ship_tax"),
            "a tuple element bound from a `for` loop over an array of literal \
             tuples written inline must resolve to every element's value: \
             {dynamic:?}"
        );
    }

    #[test]
    fn a_trigger_body_writer_is_found() {
        let migrations = r#"
            create or replace function tezgah_thing_moved() returns trigger
            language plpgsql as $$
            begin
                insert into thing_history (id, thing_id) values (gen_random_uuid(), new.id);
                return new;
            end
            $$;
        "#;
        let triggered = tables_written_by_triggers(migrations, &WRITE_PREFIXES);
        assert!(
            triggered.contains("thing_history"),
            "a table only a trigger writes must be found by reading a \
             trigger's own body in migrations/: {triggered:?}"
        );
    }

    #[test]
    fn a_non_trigger_statement_in_migrations_is_not_read_as_a_writer() {
        let migrations = r#"
            insert into tezgah_order_status_move (was, became) values ('draft', 'pending');

            create or replace function tezgah_untouched() returns void
            language plpgsql as $$
            begin
                insert into decoy_table (id) values (gen_random_uuid());
            end
            $$;
        "#;
        let triggered = tables_written_by_triggers(migrations, &WRITE_PREFIXES);
        assert!(
            triggered.is_empty(),
            "a seed insert outside any trigger, and a function that is not \
             `returns trigger`, must not count: {triggered:?}"
        );
    }
}

/// Every `check (col in ('a', 'b'))` in the migrations, as
/// `(table, column, values)`, the last definition of a column winning.
fn permitted_values(migrations: &str) -> Vec<(String, String, Vec<String>)> {
    let mut out: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut table = String::new();
    let mut at = 0;

    while at < migrations.len() {
        let create = migrations[at..].find("create table ").map(|i| at + i);
        let alter = migrations[at..].find("alter table ").map(|i| at + i);
        let check = migrations[at..].find("check (").map(|i| at + i);

        let next = [create, alter, check].into_iter().flatten().min();
        let Some(next) = next else { break };

        if Some(next) == check {
            let rest = &migrations[next + "check (".len()..];
            let column: String = rest
                .trim_start()
                .trim_start_matches('"')
                .chars()
                .take_while(|c| ident_char(*c))
                .collect();
            let after = rest
                .trim_start()
                .trim_start_matches('"')
                .trim_start_matches(&column)
                .trim_start_matches('"')
                .trim_start();
            if let Some(list) = after
                .strip_prefix("in (")
                .and_then(|list| list.find(')').map(|end| &list[..end]))
            {
                let values: Vec<String> = list
                    .split(',')
                    .filter_map(|value| {
                        let value = value.trim();
                        value
                            .strip_prefix('\'')
                            .and_then(|v| v.strip_suffix('\''))
                            .map(str::to_owned)
                    })
                    .collect();
                if !column.is_empty() && !values.is_empty() {
                    out.retain(|(t, c, _)| !(*t == table && *c == column));
                    out.push((table.clone(), column, values));
                }
            }
            at = next + "check (".len();
        } else {
            let token = if Some(next) == create {
                "create table "
            } else {
                "alter table "
            };
            table = words_after(&migrations[next..], token)
                .first()
                .cloned()
                .unwrap_or_default();
            at = next + token.len();
        }
    }

    out
}

/// Advisory, not proof: a value with no `'literal'` in `src/` may still be
/// written from SQL (a trigger, a function body) or from a caller's data
/// that this textual scan cannot follow — see the module doc. What this
/// finds is worth a human look, not a verdict.
#[test]
fn every_permitted_value_appears_as_a_literal_in_src() {
    let migrations = migrations();
    let source: String = sources()
        .into_iter()
        .map(|(_, text)| text)
        .collect::<Vec<_>>()
        .join("\n");

    let constraints = permitted_values(&migrations);
    assert!(
        constraints.len() > 20,
        "the migrations parsed to {} value constraints, which is not this schema",
        constraints.len()
    );

    let mut unwritten = Vec::new();
    let mut used = BTreeSet::new();

    for (table, column, values) in &constraints {
        for value in values {
            if source.contains(&format!("'{value}'")) || source.contains(&format!("\"{value}\"")) {
                continue;
            }
            let at = format!("{table}.{column} = '{value}'");
            match TOLERATED_VALUES.iter().find(|(known, _)| *known == at) {
                Some((known, _)) => {
                    used.insert(*known);
                }
                None => unwritten.push(at),
            }
        }
    }

    assert!(
        unwritten.is_empty(),
        "the database permits these and no literal for them appears in src/ \
         (a trigger or a caller's data could still write one — this check \
         cannot tell):\n  {}\n\
         Write them, narrow the constraint, or name them in TOLERATED_VALUES \
         with the reason.",
        unwritten.join("\n  ")
    );

    let stale: Vec<&str> = TOLERATED_VALUES
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !used.contains(name))
        .collect();
    assert!(
        stale.is_empty(),
        "these have a literal in src/ now, or the constraint changed; take \
         them out of TOLERATED_VALUES: {stale:?}"
    );
}
