//! What the two real providers must get right before a network is involved.
//!
//! Nothing here goes out. What is being tested is the part that decides whether
//! a delivery is genuine and how much money a request is actually asking for —
//! the two places where a mistake is somebody else's money.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;
use tezgah::money::{Currency, Money};
use tezgah::payment::WebhookKind;
use tezgah::providers::{Exponents, from_minor, to_decimal_string, to_minor};
use tezgah::providers::{iyzico, stripe};

const SECRET: &str = "whsec_not_a_real_key";
const IYZ_SECRET: &str = "sandbox-not-a-real-key";
const NOW: i64 = 1_700_000_000;

fn currency(code: &str) -> Currency {
    Currency::parse(code).expect("a currency code")
}

fn money(amount: Decimal, code: &str) -> Money {
    Money::new(amount, currency(code))
}

// ---------------------------------------------------------------------------
// Stripe signatures
// ---------------------------------------------------------------------------

#[test]
fn stripe_accepts_a_signature_it_would_have_made_itself() {
    let body = br#"{"id":"evt_1","type":"payment_intent.succeeded"}"#;
    let header = stripe::signature_header(SECRET, NOW, body).expect("a header");

    stripe::verify_signature(SECRET, &header, body, NOW, 300).expect("the signature holds");
}

#[test]
fn stripe_refuses_a_body_that_changed_after_it_was_signed() {
    let header = stripe::signature_header(SECRET, NOW, br#"{"id":"evt_1"}"#).expect("a header");

    assert!(stripe::verify_signature(SECRET, &header, br#"{"id":"evt_666"}"#, NOW, 300).is_err());
}

#[test]
fn stripe_refuses_a_signature_made_under_another_secret() {
    let body = br#"{"id":"evt_1"}"#;
    let header = stripe::signature_header("whsec_someone_else", NOW, body).expect("a header");

    assert!(stripe::verify_signature(SECRET, &header, body, NOW, 300).is_err());
}

#[test]
fn stripe_refuses_a_replay_of_an_old_delivery() {
    let body = br#"{"id":"evt_1"}"#;
    let header = stripe::signature_header(SECRET, NOW, body).expect("a header");

    assert!(stripe::verify_signature(SECRET, &header, body, NOW + 301, 300).is_err());
    assert!(stripe::verify_signature(SECRET, &header, body, NOW + 299, 300).is_ok());
}

#[test]
fn stripe_refuses_a_signature_that_is_shaped_wrong() {
    let body = br#"{"id":"evt_1"}"#;

    for header in ["", "v1=deadbeef", "t=1700000000", "t=soon,v1=deadbeef"] {
        assert!(
            stripe::verify_signature(SECRET, header, body, NOW, 300).is_err(),
            "{header:?} was accepted"
        );
    }
}

// ---------------------------------------------------------------------------
// Stripe events
// ---------------------------------------------------------------------------

#[test]
fn stripe_reads_a_capture_out_of_an_event() {
    let session = uuid::Uuid::now_v7().to_string();
    let payload = json!({
        "id": "evt_1",
        "type": "payment_intent.succeeded",
        "data": { "object": {
            "amount_received": 1050,
            "currency": "try",
            "metadata": { "session_id": session },
        }},
    });

    let event = stripe::read_event(&payload, &Exponents::new()).expect("an event");

    assert_eq!(event.kind, WebhookKind::Captured);
    assert_eq!(event.event_id, "evt_1");
    assert_eq!(event.amount, Some(money(dec!(10.50), "TRY")));
    assert_eq!(event.session_id.map(|id| id.to_string()), Some(session));
}

#[test]
fn stripe_is_uninterested_in_an_event_it_does_not_model() {
    let payload = json!({
        "id": "evt_2",
        "type": "customer.subscription.trial_will_end",
        "data": { "object": {} },
    });

    let event = stripe::read_event(&payload, &Exponents::new()).expect("an event, not an error");

    assert_eq!(event.kind, WebhookKind::Other);
    assert_eq!(event.event_type, "customer.subscription.trial_will_end");
}

// ---------------------------------------------------------------------------
// iyzico signatures
// ---------------------------------------------------------------------------

fn notification() -> serde_json::Value {
    json!({
        "iyziEventType": "CHECKOUTFORM_AUTH",
        "paymentId": "12345678",
        "paymentConversationId": "0192f0a0-0000-7000-8000-000000000001",
        "status": "SUCCESS",
    })
}

#[test]
fn iyzico_accepts_a_signature_over_the_fields_it_covers() {
    let payload = notification();
    let signature = iyzico::webhook_signature(IYZ_SECRET, &payload).expect("a signature");

    iyzico::verify_webhook_signature(IYZ_SECRET, &signature, &payload).expect("it holds");
}

#[test]
fn iyzico_refuses_a_notification_whose_payment_was_swapped() {
    let payload = notification();
    let signature = iyzico::webhook_signature(IYZ_SECRET, &payload).expect("a signature");

    let mut tampered = payload;
    tampered["paymentId"] = json!("87654321");

    assert!(iyzico::verify_webhook_signature(IYZ_SECRET, &signature, &tampered).is_err());
}

#[test]
fn iyzico_refuses_a_signature_made_under_another_secret() {
    let payload = notification();
    let signature = iyzico::webhook_signature("someone-elses-key", &payload).expect("a signature");

    assert!(iyzico::verify_webhook_signature(IYZ_SECRET, &signature, &payload).is_err());
}

#[test]
fn iyzico_names_a_delivery_by_what_happened_so_the_second_one_repeats_the_first() {
    let payload = notification();

    let once = iyzico::read_event(&payload).expect("an event");
    let again = iyzico::read_event(&payload).expect("an event");

    assert_eq!(once.event_id, again.event_id);
    assert_eq!(once.kind, WebhookKind::Authorized);
    assert_eq!(
        once.session_id.map(|id| id.to_string()).as_deref(),
        Some("0192f0a0-0000-7000-8000-000000000001")
    );
}

#[test]
fn iyzico_is_uninterested_in_an_event_it_does_not_model() {
    let payload = json!({
        "iyziEventType": "SOMETHING_NEW",
        "paymentId": "12345678",
        "paymentConversationId": "not-a-uuid",
        "status": "SUCCESS",
    });

    let event = iyzico::read_event(&payload).expect("an event, not an error");

    assert_eq!(event.kind, WebhookKind::Other);
    assert_eq!(event.session_id, None);
}

#[test]
fn iyzico_calls_a_failure_a_failure_whatever_the_event_was() {
    let mut payload = notification();
    payload["status"] = json!("FAILURE");

    let event = iyzico::read_event(&payload).expect("an event");

    assert_eq!(event.kind, WebhookKind::Failed);
}

#[test]
fn iyzicos_authorization_header_signs_the_body_it_sends() {
    let one = iyzico::authorization_header("api", IYZ_SECRET, "rnd", "/payment/auth", r#"{"a":1}"#)
        .expect("a header");
    let other =
        iyzico::authorization_header("api", IYZ_SECRET, "rnd", "/payment/auth", r#"{"a":2}"#)
            .expect("a header");

    assert!(one.starts_with("IYZWSv2 "));
    assert_ne!(one, other);
}

// ---------------------------------------------------------------------------
// Amounts
// ---------------------------------------------------------------------------

#[test]
fn two_decimal_places_become_a_hundredth() {
    assert_eq!(
        to_minor(money(dec!(10.50), "TRY"), &Exponents::new()).expect("minor units"),
        1050
    );
}

#[test]
fn a_currency_with_no_decimal_places_is_not_multiplied() {
    assert_eq!(
        to_minor(money(dec!(1000), "JPY"), &Exponents::new()).expect("minor units"),
        1000
    );
}

#[test]
fn a_currency_with_three_decimal_places_gets_all_three() {
    assert_eq!(
        to_minor(money(dec!(3), "KWD"), &Exponents::new()).expect("minor units"),
        3000
    );
    assert_eq!(
        to_minor(money(dec!(3.125), "KWD"), &Exponents::new()).expect("minor units"),
        3125
    );
}

#[test]
fn an_amount_survives_the_round_trip_through_minor_units() {
    let exponents = Exponents::new();
    for (amount, code) in [
        (dec!(10.50), "TRY"),
        (dec!(1000), "JPY"),
        (dec!(3.125), "KWD"),
        (dec!(0.01), "EUR"),
    ] {
        let original = money(amount, code);
        let minor = to_minor(original, &exponents).expect("minor units");
        assert_eq!(
            from_minor(minor, original.currency, &exponents).amount,
            amount,
            "{code}"
        );
    }
}

#[test]
fn a_host_that_counts_a_currency_differently_is_believed() {
    let exponents = Exponents::new().with(currency("ISK"), 2);

    assert_eq!(
        to_minor(money(dec!(10.50), "ISK"), &exponents).expect("minor units"),
        1050
    );
    // Banker's rounding: a half goes to the even neighbour, so 10.50 with no
    // decimal places is 10 and 11.50 is 12. Half-up would bias every rounding
    // in the shop's favour, which over enough orders is somebody's money.
    assert_eq!(
        to_minor(money(dec!(10.50), "ISK"), &Exponents::new()).expect("minor units"),
        10
    );
    assert_eq!(
        to_minor(money(dec!(11.50), "ISK"), &Exponents::new()).expect("minor units"),
        12
    );
}

#[test]
fn iyzicos_decimal_string_carries_the_currencys_own_places() {
    let exponents = Exponents::new();

    assert_eq!(
        to_decimal_string(money(dec!(10.5), "TRY"), &exponents),
        "10.50"
    );
    assert_eq!(
        to_decimal_string(money(dec!(1000), "JPY"), &exponents),
        "1000"
    );
    assert_eq!(
        to_decimal_string(money(dec!(3), "KWD"), &exponents),
        "3.000"
    );
}
