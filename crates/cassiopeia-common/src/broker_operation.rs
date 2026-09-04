use crate::{attribute_overwrite::AttributeOverwrite, upsert_mode::UpsertMode};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use strum::EnumDiscriminants;

/// Which NGSI-LD batch operation a broker writer performs, and the spec options attached to it.
///
/// Each variant is exactly one legal operation under ETSI GS CIM 009 v1.9.1, so an illegal
/// combination cannot be represented. Every entity operation POSTs a JSON array to
/// `ngsi-ld/v1/entityOperations/{verb}`; the temporal operation is strictly per-entity and has no
/// batch form in v1.9.1.
///
/// Spec options ride on the variant the spec attaches them to: the upsert mode on
/// [`Upsert`](BrokerOperation::Upsert) (clause 5.6.8), the attribute-overwrite flag on
/// [`Update`](BrokerOperation::Update) (clause 5.6.9).
///
/// [`BrokerOperationKind`] is the flat discriminant the CLI and manifest carry, since `clap`'s
/// `ValueEnum` and plain serde cannot express the option payloads; [`BrokerOperation::from_parts`]
/// reassembles the full value at the composition root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumDiscriminants)]
#[strum_discriminants(name(BrokerOperationKind))]
#[strum_discriminants(derive(Hash, Serialize, Deserialize, Default, ValueEnum))]
#[strum_discriminants(serde(rename_all = "kebab-case"))]
pub enum BrokerOperation {
    /// Batch upsert to `entityOperations/upsert` (clause 5.6.8). The default operation.
    #[strum_discriminants(default)]
    Upsert(UpsertMode),
    /// Batch create to `entityOperations/create` (clause 5.6.7).
    Create,
    /// Batch update to `entityOperations/update` (clause 5.6.9).
    Update(AttributeOverwrite),
    /// Batch merge to `entityOperations/merge` (clause 5.6.20).
    Merge,
    /// Temporal upsert to `temporal/entities` (clause 5.6.11); one entity per request, no batch form.
    Temporal,
}

impl BrokerOperation {
    /// Reassembles the full operation from the flat kind the CLI and manifest carry plus the two
    /// option enums.
    ///
    /// The option that does not belong to the chosen kind is ignored, so passing an upsert mode to a
    /// [`Create`](BrokerOperationKind::Create) simply yields [`Create`](BrokerOperation::Create).
    #[must_use]
    pub const fn from_parts(kind: BrokerOperationKind, upsert_mode: UpsertMode, attribute_overwrite: AttributeOverwrite) -> BrokerOperation {
        match kind {
            BrokerOperationKind::Upsert => BrokerOperation::Upsert(upsert_mode),
            BrokerOperationKind::Create => BrokerOperation::Create,
            BrokerOperationKind::Update => BrokerOperation::Update(attribute_overwrite),
            BrokerOperationKind::Merge => BrokerOperation::Merge,
            BrokerOperationKind::Temporal => BrokerOperation::Temporal,
        }
    }
}

impl Default for BrokerOperation {
    /// The default operation is a replacing batch upsert, matching the spec's default upsert mode
    /// (clause 5.6.8). A manual impl is required because the derived `Default` cannot select a
    /// data-carrying variant.
    fn default() -> BrokerOperation {
        BrokerOperation::Upsert(UpsertMode::Replace)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_overwrite::AttributeOverwrite,
        broker_operation::{BrokerOperation, BrokerOperationKind},
        upsert_mode::UpsertMode,
    };

    #[test]
    fn the_default_operation_is_a_replacing_upsert() {
        assert_eq!(BrokerOperation::default(), BrokerOperation::Upsert(UpsertMode::Replace));
    }

    #[test]
    fn every_kind_round_trips_through_its_kebab_case_token() {
        for (kind, token) in [
            (BrokerOperationKind::Upsert, r#""upsert""#),
            (BrokerOperationKind::Create, r#""create""#),
            (BrokerOperationKind::Update, r#""update""#),
            (BrokerOperationKind::Merge, r#""merge""#),
            (BrokerOperationKind::Temporal, r#""temporal""#),
        ] {
            assert_eq!(serde_json::to_string(&kind).unwrap(), token);
            assert_eq!(serde_json::from_str::<BrokerOperationKind>(token).unwrap(), kind);
        }
    }

    #[test]
    fn the_default_kind_is_upsert() {
        assert_eq!(BrokerOperationKind::default(), BrokerOperationKind::Upsert);
    }

    #[test]
    fn from_parts_threads_the_upsert_mode_into_an_upsert() {
        assert_eq!(
            BrokerOperation::from_parts(BrokerOperationKind::Upsert, UpsertMode::Update, AttributeOverwrite::Overwrite),
            BrokerOperation::Upsert(UpsertMode::Update)
        );
    }

    #[test]
    fn from_parts_threads_the_attribute_overwrite_into_an_update() {
        assert_eq!(
            BrokerOperation::from_parts(BrokerOperationKind::Update, UpsertMode::Replace, AttributeOverwrite::NoOverwrite),
            BrokerOperation::Update(AttributeOverwrite::NoOverwrite)
        );
    }

    #[test]
    fn from_parts_ignores_the_options_for_optionless_kinds() {
        assert_eq!(
            BrokerOperation::from_parts(BrokerOperationKind::Create, UpsertMode::Update, AttributeOverwrite::NoOverwrite),
            BrokerOperation::Create
        );
        assert_eq!(
            BrokerOperation::from_parts(BrokerOperationKind::Merge, UpsertMode::Update, AttributeOverwrite::NoOverwrite),
            BrokerOperation::Merge
        );
        assert_eq!(
            BrokerOperation::from_parts(BrokerOperationKind::Temporal, UpsertMode::Update, AttributeOverwrite::NoOverwrite),
            BrokerOperation::Temporal
        );
    }
}
