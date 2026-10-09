use std::collections::{BTreeMap, HashMap};

use thiserror::Error;

use crate::{Account, AccountBalance};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    pub source: Account,
    pub target: Account,
    pub amount_minor: u64,
}

pub type Settlement = Vec<Transfer>;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SettlementError {
    #[error("before and target states must contain the same account names")]
    AccountSetMismatch,
    #[error("account name {0} appears more than once")]
    DuplicateAccount(String),
    #[error("total balance changes by {0} minor units")]
    Unbalanced(i128),
    #[error("transfer amount exceeds supported range")]
    AmountOverflow,
    #[error("account {0} is missing from the discovered accounts")]
    MissingAccount(String),
}

/// Computes an exact minimum-cardinality settlement for one conserved currency.
///
/// The search pairs one remaining surplus with every compatible deficit, always
/// transferring the maximum possible amount. That is sufficient for an exact
/// solution because an optimal solution can be transformed to close one side
/// of each chosen pair. Memoization and a greedy upper bound keep small account
/// sets practical.
///
/// # Errors
///
/// Returns an error for mismatched or duplicate account names, an unbalanced
/// target, or an amount outside the transfer range.
pub fn settle<'account>(
    initial: &[AccountBalance<'account>],
    target: &[AccountBalance<'account>],
    accounts: &[Account],
) -> Result<Settlement, SettlementError> {
    let initial = state_map(initial)?;
    let target = state_map(target)?;
    if initial.keys().ne(target.keys()) {
        return Err(SettlementError::AccountSetMismatch);
    }
    let mut deltas = Vec::with_capacity(initial.len());
    let mut total_delta = 0_i128;
    for (name, current) in &initial {
        let desired = *target
            .get(name)
            .ok_or(SettlementError::AccountSetMismatch)?;
        let delta = i128::from(*current)
            .checked_sub(i128::from(desired))
            .ok_or(SettlementError::AmountOverflow)?;
        total_delta = total_delta
            .checked_add(delta)
            .ok_or(SettlementError::AmountOverflow)?;
        deltas.push((*name, delta));
    }
    if total_delta != 0 {
        return Err(SettlementError::Unbalanced(total_delta));
    }

    let account_map = accounts
        .iter()
        .map(|account| (account.name.as_str(), account))
        .collect::<BTreeMap<_, _>>();
    for name in initial.keys() {
        if !account_map.contains_key(name) {
            return Err(SettlementError::MissingAccount((*name).to_owned()));
        }
    }

    let mut memo = HashMap::new();
    let mut best = None;
    search(&deltas, &mut Vec::new(), &mut best, &mut memo)?;
    best.unwrap_or_default()
        .into_iter()
        .map(|transfer| {
            let source = account_map
                .get(transfer.source)
                .ok_or_else(|| SettlementError::MissingAccount(transfer.source.to_owned()))?;
            let target = account_map
                .get(transfer.target)
                .ok_or_else(|| SettlementError::MissingAccount(transfer.target.to_owned()))?;
            Ok(Transfer {
                source: (*source).clone(),
                target: (*target).clone(),
                amount_minor: transfer.amount_minor,
            })
        })
        .collect()
}

#[derive(Clone)]
struct PlannedTransfer<'account> {
    source: &'account str,
    target: &'account str,
    amount_minor: u64,
}

fn search<'account>(
    state: &[(&'account str, i128)],
    transfers: &mut Vec<PlannedTransfer<'account>>,
    best: &mut Option<Vec<PlannedTransfer<'account>>>,
    memo: &mut HashMap<Vec<i128>, usize>,
) -> Result<(), SettlementError> {
    if state.iter().all(|(_, delta)| *delta == 0) {
        if best
            .as_ref()
            .is_none_or(|candidate| transfers.len() < candidate.len())
        {
            *best = Some(transfers.clone());
        }
        return Ok(());
    }
    if best
        .as_ref()
        .is_some_and(|candidate| transfers.len() >= candidate.len())
    {
        return Ok(());
    }

    let key: Vec<_> = state.iter().map(|(_, delta)| *delta).collect();
    if memo
        .get(&key)
        .is_some_and(|depth| *depth <= transfers.len())
    {
        return Ok(());
    }
    memo.insert(key, transfers.len());

    let source_index = state
        .iter()
        .position(|(_, delta)| *delta != 0)
        .ok_or(SettlementError::AmountOverflow)?;
    let source_delta = state
        .get(source_index)
        .ok_or(SettlementError::AmountOverflow)?
        .1;
    let source_is_surplus = source_delta > 0;
    for destination_index in 0..state.len() {
        if destination_index == source_index {
            continue;
        }
        let destination_delta = state
            .get(destination_index)
            .ok_or(SettlementError::AmountOverflow)?
            .1;
        if (destination_delta > 0) == source_is_surplus || destination_delta == 0 {
            continue;
        }
        let amount = source_delta
            .unsigned_abs()
            .min(destination_delta.unsigned_abs());
        let amount = u64::try_from(amount).map_err(|_| SettlementError::AmountOverflow)?;
        let mut next = state.to_vec();
        let source_next = next
            .get_mut(source_index)
            .ok_or(SettlementError::AmountOverflow)?;
        source_next.1 = if source_is_surplus {
            source_delta
                .checked_sub(i128::from(amount))
                .ok_or(SettlementError::AmountOverflow)?
        } else {
            source_delta
                .checked_add(i128::from(amount))
                .ok_or(SettlementError::AmountOverflow)?
        };
        let destination_next = next
            .get_mut(destination_index)
            .ok_or(SettlementError::AmountOverflow)?;
        destination_next.1 = if source_is_surplus {
            destination_delta
                .checked_add(i128::from(amount))
                .ok_or(SettlementError::AmountOverflow)?
        } else {
            destination_delta
                .checked_sub(i128::from(amount))
                .ok_or(SettlementError::AmountOverflow)?
        };
        let source_name = state
            .get(source_index)
            .ok_or(SettlementError::AmountOverflow)?
            .0;
        let destination_name = state
            .get(destination_index)
            .ok_or(SettlementError::AmountOverflow)?
            .0;
        let (source, target) = if source_is_surplus {
            (source_name, destination_name)
        } else {
            (destination_name, source_name)
        };
        transfers.push(PlannedTransfer {
            source,
            target,
            amount_minor: amount,
        });
        search(&next, transfers, best, memo)?;
        transfers.pop();
    }
    Ok(())
}

fn state_map<'account>(
    state: &[AccountBalance<'account>],
) -> Result<BTreeMap<&'account str, u64>, SettlementError> {
    let mut result = BTreeMap::new();
    for balance in state {
        if result.insert(balance.0, balance.1).is_some() {
            return Err(SettlementError::DuplicateAccount(balance.0.to_owned()));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AccountKind;

    fn balances<'a>(values: &'a [(&'a str, u64)]) -> Vec<AccountBalance<'a>> {
        values.to_vec()
    }

    fn accounts(names: &[&str]) -> Vec<Account> {
        names
            .iter()
            .map(|name| Account {
                name: (*name).to_owned(),
                iban: format!("NL00{name}"),
                kind: AccountKind::Bank,
                user_id: 1,
                monetary_account_id: 1,
                currency: "EUR".to_owned(),
            })
            .collect()
    }

    #[test]
    fn finds_the_exact_three_transfer_solution() {
        let settlement = settle(
            &balances(&[("a", 4), ("b", 3), ("c", 2), ("d", 0), ("e", 0)]),
            &balances(&[("a", 0), ("b", 0), ("c", 0), ("d", 5), ("e", 4)]),
            &accounts(&["a", "b", "c", "d", "e"]),
        )
        .expect("states are balanced");

        assert_eq!(settlement.len(), 3);
    }

    #[test]
    fn external_nodes_only_receive_from_the_convention() {
        let initial = vec![("checking", 100), ("external", 0)];
        let target = vec![("checking", 40), ("external", 60)];
        let mut account_list = accounts(&["checking"]);
        account_list.push(Account {
            name: "external".to_owned(),
            iban: "NL00external".to_owned(),
            kind: AccountKind::External,
            user_id: 1,
            monetary_account_id: 0,
            currency: String::new(),
        });
        let settlement =
            settle(&initial, &target, &account_list).expect("external sink receives money");

        assert_eq!(settlement.len(), 1);
        assert_eq!(settlement[0].target.name, "external");
    }
}
