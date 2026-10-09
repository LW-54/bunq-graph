use bunq_api::{Client, Iban, PaymentRequest};
use thiserror::Error;

use crate::AccountKind;

#[derive(Debug, Error)]
pub enum PaymentError {
    #[error("external account {0} cannot send payments")]
    ExternalSource(String),
    #[error("payment amount is too large")]
    AmountOverflow,
    #[error("could not construct bunq payment: {0}")]
    InvalidRequest(#[source] Box<bunq_api::Error>),
    #[error("bunq payment failed for {name}: {source}")]
    Bunq {
        name: String,
        #[source]
        source: Box<bunq_api::Error>,
    },
    #[error("invalid destination IBAN for {name}: {source}")]
    InvalidIban {
        name: String,
        #[source]
        source: Box<bunq_api::Error>,
    },
}

/// Executes a settlement as one-shot bunq payments in deterministic order.
///
/// The bunq SDK does not retry payment writes. An error is returned immediately
/// so the caller can inspect payment history before deciding on recovery.
///
/// Executes one settlement transfer as a bunq payment.
///
/// # Errors
///
/// Returns an error when an account is not bound, the request is invalid, or
/// bunq rejects the payment.
pub async fn execute(client: &Client, transfer: &crate::Transfer) -> Result<(), PaymentError> {
    if transfer.source.kind == AccountKind::External {
        return Err(PaymentError::ExternalSource(transfer.source.name.clone()));
    }
    let amount = i64::try_from(transfer.amount_minor).map_err(|_| PaymentError::AmountOverflow)?;
    let iban = Iban::parse(&transfer.target.iban).map_err(|error| PaymentError::InvalidIban {
        name: transfer.target.name.clone(),
        source: Box::new(error),
    })?;
    let amount_value = format!("{}.{:02}", amount / 100, amount % 100);
    let request = PaymentRequest::new_iban(
        amount_value,
        &transfer.source.currency,
        &iban,
        Some(format!(
            "bunq-graph: {} -> {}",
            transfer.source.name, transfer.target.name
        )),
    )
    .map_err(|error| PaymentError::InvalidRequest(Box::new(error)))?
    .with_counterparty_name(transfer.target.name.clone());
    client
        .create_payment(
            transfer.source.user_id,
            transfer.source.monetary_account_id,
            &request,
        )
        .await
        .map_err(|error| PaymentError::Bunq {
            name: transfer.source.name.clone(),
            source: Box::new(error),
        })
        .map(|_| ())
}
