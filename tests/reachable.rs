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
//! 2. every table a migration creates has a writer in `src/`;
//! 3. every value a `check (col in (..))` permits appears as a `'literal'`
//!    somewhere in `src/`.
//!
//! What this does not prove: that a caller is on a path a request can reach,
//! or that a route is wired up by the host, or that the value written is
//! written in the right column. Names are matched textually — a caller is a
//! `module::name(` elsewhere, or a bare `name(` in a file that imported it —
//! so two functions sharing a name share their callers.
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
        "credit::refund_to_credit",
        "store credit has no route and no in-crate caller; shipped unwired, see \
         #108",
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
        "inventory::locations_for_sales_channel",
        "sales channels are read nowhere and reach no route; shipped unwired, see \
         #109",
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
        "inventory::reserve_from_lot",
        "lot and serial tracking has no route and no in-crate caller; shipped \
         unwired, see #110",
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
        "payment::save_account_holder",
        "a customer as one provider knows them; nothing saves or reads a saved \
         card yet",
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
        "store::sales_channel",
        "sales channels are read nowhere and reach no route; shipped unwired, see \
         #109",
    ),
    (
        "store::publishable_key",
        "sales channels are read nowhere and reach no route; shipped unwired, see \
         #109",
    ),
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
const TOLERATED_TABLES: [(&str, &str); 6] = [
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

#[test]
fn every_table_has_a_writer() {
    let migrations = migrations();
    let source: String = sources()
        .into_iter()
        .map(|(_, text)| text)
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

    let mut unwritten = Vec::new();
    let mut used = BTreeSet::new();

    for table in &tables {
        let written = [
            format!("insert into {table}"),
            format!("update {table}"),
            format!("delete from {table}"),
            format!("\"{table}\""),
        ]
        .iter()
        .any(|form| source.contains(form.as_str()));
        if written {
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
