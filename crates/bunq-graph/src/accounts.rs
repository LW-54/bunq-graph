use bunq_api::{Client, Error as BunqError, MonetaryAccountBank, MonetaryAccountSavings};
use serde_json::Value;
use thiserror::Error;

use crate::Bindings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub name: String,
    pub iban: String,
    pub kind: AccountKind,
    pub user_id: u64,
    pub monetary_account_id: u64,
    pub currency: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountKind {
    Bank,
    Savings,
    External,
}

#[derive(Debug, Error)]
pub enum AccountError {
    #[error("bunq request failed: {0}")]
    Bunq(#[source] Box<bunq_api::Error>),
    #[error("managed node {0} could not be matched to a bunq account by IBAN")]
    MissingIban(String),
    #[error("managed node {0} matches more than one bunq account")]
    DuplicateIban(String),
    #[error("account {name} has no balance")]
    MissingBalance { name: String },
    #[error("account {name} returned invalid balance {value}")]
    InvalidBalance { name: String, value: String },
    #[error("managed accounts use multiple currencies: {first} and {second}")]
    MixedCurrencies { first: String, second: String },
    #[error("managed account {name} has no currency")]
    MissingCurrency { name: String },
}

#[derive(Debug)]
struct Candidate {
    id: u64,
    currency: String,
    iban: String,
    kind: AccountKind,
}

pub type AccountBalance<'account> = (&'account str, u64);

/// Discovers managed bunq accounts by binding IBAN and preserves external
/// bindings as sink-only accounts.
///
/// # Errors
///
/// Returns an error when bunq cannot be queried or a managed IBAN cannot be
/// matched uniquely.
pub async fn discover_accounts(
    client: &Client,
    user_id: u64,
    bindings: &Bindings,
) -> Result<Vec<Account>, AccountError> {
    let mut candidates = Vec::new();
    for account in list_banks(client, user_id).await? {
        candidates.push(candidate_from_bank(&account));
    }
    for account in list_savings(client, user_id).await? {
        candidates.push(candidate_from_savings(&account));
    }
    bindings
        .account
        .iter()
        .map(|binding| {
            if binding.external {
                return Ok(Account {
                    name: binding.name.clone(),
                    iban: binding.iban.clone(),
                    kind: AccountKind::External,
                    user_id,
                    monetary_account_id: 0,
                    currency: String::new(),
                });
            }
            let matches: Vec<_> = candidates
                .iter()
                .filter(|candidate| candidate.iban == binding.iban)
                .collect();
            let candidate = match matches.as_slice() {
                [candidate] => candidate,
                [] => return Err(AccountError::MissingIban(binding.name.clone())),
                _ => return Err(AccountError::DuplicateIban(binding.name.clone())),
            };
            Ok(Account {
                name: binding.name.clone(),
                iban: binding.iban.clone(),
                kind: candidate.kind,
                user_id,
                monetary_account_id: candidate.id,
                currency: candidate.currency.clone(),
            })
        })
        .collect()
}

async fn list_banks(
    client: &Client,
    user_id: u64,
) -> Result<Vec<MonetaryAccountBank>, AccountError> {
    match client.list_monetary_account_banks(user_id).await {
        Ok(response) => Ok(response.response),
        Err(error) if is_empty_response(&error) => Ok(Vec::new()),
        Err(error) => Err(AccountError::Bunq(Box::new(error))),
    }
}

async fn list_savings(
    client: &Client,
    user_id: u64,
) -> Result<Vec<MonetaryAccountSavings>, AccountError> {
    match client.list_monetary_account_savings(user_id).await {
        Ok(response) => Ok(response.response),
        Err(error) if is_empty_response(&error) => Ok(Vec::new()),
        Err(error) => Err(AccountError::Bunq(Box::new(error))),
    }
}

fn is_empty_response(error: &BunqError) -> bool {
    matches!(error, BunqError::InvalidResponse(message) if message == "Response array must contain at least one item")
}

/// Reads managed balances and initializes external nodes to zero for this run.
///
/// # Errors
///
/// Returns an error when bunq returns a missing or malformed balance.
pub async fn read_balances<'account>(
    client: &Client,
    accounts: &'account [Account],
) -> Result<Vec<AccountBalance<'account>>, AccountError> {
    validate_currencies(accounts)?;

    macro_rules! read_account {
        ($request:expr) => {{
            let mut attempts: u8 = 0;
            loop {
                match $request.await {
                    Ok(response) => break response,
                    Err(error)
                        if matches!(error, BunqError::MissingResponseSignature) && attempts < 3 =>
                    {
                        attempts = attempts.saturating_add(1);
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                    Err(error) => return Err(AccountError::Bunq(Box::new(error))),
                }
            }
        }};
    }

    let mut balances = Vec::with_capacity(accounts.len());
    for account in accounts {
        if account.kind == AccountKind::External {
            balances.push((account.name.as_str(), 0));
            continue;
        }
        let balance = match account.kind {
            AccountKind::Bank => read_account!(
                client.get_monetary_account_bank(account.user_id, account.monetary_account_id)
            )
            .response
            .first()
            .and_then(|account| account.balance.as_ref())
            .cloned(),
            AccountKind::Savings => read_account!(
                client.get_monetary_account_savings(account.user_id, account.monetary_account_id)
            )
            .response
            .first()
            .and_then(|account| account.balance.as_ref())
            .cloned(),
            AccountKind::External => None,
        };
        let balance = balance
            .as_ref()
            .ok_or_else(|| AccountError::MissingBalance {
                name: account.name.clone(),
            })?;
        let amount = if balance.value.parse::<f64>().is_ok_and(|value| value == 0.0) {
            0
        } else {
            let money = bunq_api::Money::from_decimal(&balance.currency, &balance.value, 2)
                .map_err(|_| AccountError::InvalidBalance {
                    name: account.name.clone(),
                    value: balance.value.clone(),
                })?;
            u64::try_from(money.minor_units).map_err(|_| AccountError::InvalidBalance {
                name: account.name.clone(),
                value: balance.value.clone(),
            })?
        };
        balances.push((account.name.as_str(), amount));
    }
    Ok(balances)
}

fn validate_currencies(accounts: &[Account]) -> Result<(), AccountError> {
    let mut currency: Option<String> = None;
    for account in accounts
        .iter()
        .filter(|account| account.kind != AccountKind::External)
    {
        if account.currency.is_empty() {
            return Err(AccountError::MissingCurrency {
                name: account.name.clone(),
            });
        }
        if let Some(first) = &currency {
            if first != &account.currency {
                return Err(AccountError::MixedCurrencies {
                    first: first.clone(),
                    second: account.currency.clone(),
                });
            }
        } else {
            currency = Some(account.currency.clone());
        }
    }
    Ok(())
}

fn candidate_from_bank(account: &MonetaryAccountBank) -> Candidate {
    Candidate {
        iban: find_iban(&account.additional),
        id: account.id,
        currency: account
            .balance
            .as_ref()
            .map_or_else(String::new, |balance| balance.currency.clone()),
        kind: AccountKind::Bank,
    }
}

fn candidate_from_savings(account: &MonetaryAccountSavings) -> Candidate {
    Candidate {
        iban: find_iban(&account.additional),
        id: account.id,
        currency: account
            .balance
            .as_ref()
            .map_or_else(String::new, |balance| balance.currency.clone()),
        kind: AccountKind::Savings,
    }
}

fn find_iban(value: &std::collections::HashMap<String, Value>) -> String {
    value.values().find_map(find_iban_value).unwrap_or_default()
}

fn find_iban_value(value: &Value) -> Option<String> {
    match value {
        Value::Object(object) => {
            if object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.eq_ignore_ascii_case("IBAN"))
                && let Some(value) = object.get("value").and_then(Value::as_str)
            {
                return Some(value.to_owned());
            }
            object.iter().find_map(|(key, value)| {
                if key.eq_ignore_ascii_case("iban") {
                    value.as_str().map(str::to_owned)
                } else {
                    find_iban_value(value)
                }
            })
        }
        Value::Array(values) => values.iter().find_map(find_iban_value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Account, AccountError, AccountKind, validate_currencies};
    fn account(name: &str, currency: &str) -> Account {
        Account {
            name: name.to_owned(),
            iban: String::new(),
            kind: AccountKind::Bank,
            user_id: 1,
            monetary_account_id: 1,
            currency: currency.to_owned(),
        }
    }

    #[test]
    fn rejects_mixed_currencies() {
        let error = validate_currencies(&[account("eur", "EUR"), account("usd", "USD")])
            .expect_err("mixed currencies must be rejected");

        assert!(matches!(
            error,
            AccountError::MixedCurrencies { first, second }
                if first == "EUR" && second == "USD"
        ));
    }
}
