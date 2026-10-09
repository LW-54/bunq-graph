mod accounts;
mod auth;
mod bindings;
mod payments;
mod planning;

pub use accounts::{
    Account, AccountBalance, AccountError, AccountKind, discover_accounts, read_balances,
};
pub use auth::{AuthError, ConnectedClient, connect, persisted_json};
pub use bindings::{Binding, BindingError, Bindings};
pub use payments::{PaymentError, execute};
pub use planning::{Settlement, SettlementError, Transfer, settle};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_balances_need_no_transfers() {
        let plan = settle(
            &[("reserve", 1_000)],
            &[("reserve", 1_000)],
            &[Account {
                name: "reserve".to_owned(),
                iban: "NL00reserve".to_owned(),
                kind: AccountKind::Bank,
                user_id: 1,
                monetary_account_id: 1,
                currency: "EUR".to_owned(),
            }],
        )
        .expect("equal states are valid");

        assert_eq!(plan.len(), 0);
    }
}
