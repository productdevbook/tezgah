//! Typed identifiers, so a product id cannot be passed where an order id goes.
//!
//! All of them wrap a UUIDv7: sortable by creation time, which makes an index
//! on one behave, while staying unguessable in a way a sequence is not.
//!
//! # Examples
//!
//! ```
//! use tezgah::id::{OrderId, ProductId};
//!
//! let order = OrderId::new();
//! let product = ProductId::new();
//! # fn ship(_: OrderId) {}
//! ship(order);
//! // ship(product) would not compile.
//! ```

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{Error, Result};

macro_rules! ids {
    ($($name:ident => $label:literal),* $(,)?) => {$(
        #[doc = concat!("Identifies one ", $label, ".")]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// A fresh id, ordered after every id made before it.
            pub fn new() -> Self {
                $name(Uuid::now_v7())
            }

            pub const fn from_uuid(raw: Uuid) -> Self {
                $name(raw)
            }

            pub const fn as_uuid(self) -> Uuid {
                self.0
            }

            /// What this kind of id is called, for error messages.
            pub const fn label() -> &'static str {
                $label
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Uuid {
                id.0
            }
        }

        impl FromStr for $name {
            type Err = Error;

            fn from_str(text: &str) -> Result<Self> {
                Uuid::parse_str(text)
                    .map($name)
                    .map_err(|_| Error::invalid(format!("{text:?} is not a {} id", $label)))
            }
        }

        impl sqlx::Type<sqlx::Postgres> for $name {
            fn type_info() -> sqlx::postgres::PgTypeInfo {
                <Uuid as sqlx::Type<sqlx::Postgres>>::type_info()
            }
        }

        impl<'r> sqlx::Decode<'r, sqlx::Postgres> for $name {
            fn decode(
                value: sqlx::postgres::PgValueRef<'r>,
            ) -> std::result::Result<Self, sqlx::error::BoxDynError> {
                <Uuid as sqlx::Decode<sqlx::Postgres>>::decode(value).map($name)
            }
        }

        impl<'q> sqlx::Encode<'q, sqlx::Postgres> for $name {
            fn encode_by_ref(
                &self,
                buf: &mut sqlx::postgres::PgArgumentBuffer,
            ) -> std::result::Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                <Uuid as sqlx::Encode<sqlx::Postgres>>::encode_by_ref(&self.0, buf)
            }
        }
    )*};
}

ids! {
    ProductId => "product",
    VariantId => "variant",
    OptionId => "option",
    OptionValueId => "option value",
    CollectionId => "collection",
    CategoryId => "category",
    PriceSetId => "price set",
    PriceId => "price",
    PriceListId => "price list",
    InventoryItemId => "inventory item",
    InventoryLevelId => "inventory level",
    InventoryLotId => "inventory lot",
    ReservationId => "reservation",
    LocationId => "location",
    CartId => "cart",
    LineItemId => "line item",
    OrderId => "order",
    OrderItemId => "order item",
    OrderChangeId => "order change",
    OrderTransferId => "order transfer",
    AgreementVersionId => "agreement version",
    OrderAgreementId => "order agreement",
    PaymentCollectionId => "payment collection",
    PaymentSessionId => "payment session",
    PaymentProviderId => "payment provider",
    PaymentWebhookEventId => "payment webhook event",
    PaymentId => "payment",
    CaptureId => "capture",
    RefundId => "refund",
    RefundReasonId => "refund reason",
    FulfillmentId => "fulfillment",
    ShippingOptionId => "shipping option",
    CustomerId => "customer",
    CustomerGroupId => "customer group",
    AddressId => "address",
    PromotionId => "promotion",
    TaxRegionId => "tax region",
    TaxRateId => "tax rate",
    RegionId => "region",
    SalesChannelId => "sales channel",
    StoreId => "store",
    PublishableKeyId => "publishable key",
    StockLocationId => "stock location",
    ReturnId => "return",
    ExchangeId => "exchange",
    ClaimId => "claim",
    CampaignId => "campaign",
    GiftCardId => "gift card",
    GiftCardTransactionId => "gift card transaction",
    StoreCreditId => "store credit",
    StoreCreditTransactionId => "store credit transaction",
    CartCreditId => "cart credit",
    ShippingProfileId => "shipping profile",
    ServiceZoneId => "service zone",
    GeoZoneId => "geo zone",
    FulfillmentSetId => "fulfillment set",
    ProductTagId => "tag",
    ProductTypeId => "type",
    ProductImageId => "image",
    AdjustmentId => "adjustment",
    TaxLineId => "tax line",
    ShippingMethodId => "shipping method",
    AccountHolderId => "account holder",
    DigitalContentId => "digital content",
    OrderEntitlementId => "entitlement",
    EntitlementTokenId => "entitlement token",
    SellingPlanGroupId => "selling plan group",
    SellingPlanId => "selling plan",
    SubscriptionId => "subscription",
    SubscriptionLineId => "subscription line",
    SubscriptionOrderId => "subscription order",
    SubscriptionEventId => "subscription event",
    OrderInvoiceId => "order invoice",
    OrderTransactionId => "order transaction",
    WorkflowRunId => "workflow run",
    BundleComponentId => "bundle component",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_made_later_sort_after_ids_made_earlier() {
        let first = OrderId::new();
        let second = OrderId::new();
        assert!(first < second);
    }

    #[test]
    fn a_uuid_survives_the_round_trip_through_text() {
        let id = ProductId::new();
        let back: ProductId = id.to_string().parse().expect("its own text parses");
        assert_eq!(id, back);
    }

    #[test]
    fn text_that_is_not_a_uuid_is_refused_by_name() {
        let err = "not-a-uuid".parse::<CartId>().expect_err("refuses");
        assert!(err.to_string().contains("cart"));
    }
}
