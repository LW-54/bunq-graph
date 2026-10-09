use std::time::Duration;

use bunq_api::{
    Client, CounterpartyAlias, MonetaryAccountBankRequest, MonetaryAccountSavingsRequest,
    RequestInquiryRequest, SandboxUserKind, create_sandbox_user, install_device,
};
use bunq_graph::{Binding, Bindings, discover_accounts, execute, read_balances, settle};
use clepsydra_lib::clepsydra_eval;
use serde_json::Value;

#[tokio::test]
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::similar_names,
    clippy::too_many_lines
)]
async fn sandbox_routes_between_two_disposable_accounts() {
    if std::env::var("SANDBOX_TESTS").as_deref() != Ok("1") {
        return;
    }

    let (source_client, source_user, source_account_id, source_iban) =
        provision_funded_account().await;
    let (destination_client, destination_user, destination_account_id, destination_iban) =
        provision_funded_account().await;

    let source_bindings = Bindings {
        account: vec![Binding {
            name: "checking".to_owned(),
            iban: source_iban,
            external: false,
        }],
    };
    let destination_bindings = Bindings {
        account: vec![Binding {
            name: "savings".to_owned(),
            iban: destination_iban,
            external: false,
        }],
    };
    let source_accounts = discover_accounts(&source_client, source_user, &source_bindings)
        .await
        .expect("source binding should resolve");
    let destination_accounts =
        discover_accounts(&destination_client, destination_user, &destination_bindings)
            .await
            .expect("destination binding should resolve");
    let source_account = source_accounts.into_iter().next().expect("source account");
    let destination_account = destination_accounts
        .into_iter()
        .next()
        .expect("destination account");
    assert_eq!(source_account.monetary_account_id, source_account_id);
    assert_eq!(
        destination_account.monetary_account_id,
        destination_account_id
    );

    let source_initial = read_balances(&source_client, std::slice::from_ref(&source_account))
        .await
        .expect("source balance should be readable")[0]
        .1;
    let destination_initial = read_balances(
        &destination_client,
        std::slice::from_ref(&destination_account),
    )
    .await
    .expect("destination balance should be readable")[0]
        .1;
    assert!(source_initial >= 100);
    assert!(destination_initial >= 100);

    let evaluate = clepsydra_eval!(
        r#"
        [nodes.checking]

        [nodes.savings]

        [[flow]]
        source = "checking"
        target = "savings"
        volume = 50
        "#
    );
    let initial = [
        ("checking", source_initial),
        ("savings", destination_initial),
    ];
    let target = evaluate(initial).expect("valid Clepsydra state");
    let accounts = [source_account, destination_account];
    let settlement =
        settle(&initial, &target, &accounts).expect("two-account graph should conserve value");
    assert_eq!(settlement.len(), 1);
    assert_eq!(settlement[0].source.name, "checking");
    assert_eq!(settlement[0].target.name, "savings");
    assert_eq!(settlement[0].amount_minor, 50);

    if std::env::var("SANDBOX_PAYMENTS").as_deref() == Ok("1") {
        execute(&source_client, &settlement[0])
            .await
            .expect("sandbox routing payment should succeed");

        let mut routed = false;
        for _ in 0..10 {
            let destination_current =
                read_balances(&destination_client, std::slice::from_ref(&accounts[1]))
                    .await
                    .expect("destination balance should remain readable")[0]
                    .1;
            if destination_current >= destination_initial + 50 {
                routed = true;
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        assert!(routed, "sandbox routed payment did not arrive");
    }
}

#[tokio::test]
#[allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines
)]
async fn sandbox_one_user_creates_accounts_and_routes_between_them() {
    if std::env::var("SANDBOX_TESTS").as_deref() != Ok("1") {
        return;
    }

    let (client, user_id, source_id, source_iban) = provision_funded_account().await;
    let mut bank_request = MonetaryAccountBankRequest::new("EUR").expect("bank currency");
    bank_request.description = Some("bunq-graph created bank".to_owned());
    let created_bank = client
        .create_monetary_account_bank(user_id, &bank_request)
        .await
        .expect("created bank account should succeed")
        .response[0]
        .id;
    let created_bank_detail = client
        .get_monetary_account_bank(user_id, created_bank)
        .await
        .expect("created bank detail should succeed")
        .response[0]
        .clone();
    let created_bank_iban =
        find_iban(&created_bank_detail.additional).expect("created bank should expose an IBAN");

    let mut savings_request = MonetaryAccountSavingsRequest::new("EUR").expect("savings currency");
    savings_request.description = Some("bunq-graph created savings".to_owned());
    let created_savings = client
        .create_monetary_account_savings(user_id, &savings_request)
        .await
        .expect("created savings account should succeed")
        .response[0]
        .id;
    let created_savings_detail = client
        .get_monetary_account_savings(user_id, created_savings)
        .await
        .expect("created savings detail should succeed")
        .response[0]
        .clone();
    let created_savings_iban = find_iban(&created_savings_detail.additional)
        .expect("created savings should expose an IBAN");

    let bindings = Bindings {
        account: vec![
            Binding {
                name: "source".to_owned(),
                iban: source_iban,
                external: false,
            },
            Binding {
                name: "created-bank".to_owned(),
                iban: created_bank_iban,
                external: false,
            },
            Binding {
                name: "created-savings".to_owned(),
                iban: created_savings_iban,
                external: false,
            },
        ],
    };
    let accounts = discover_accounts(&client, user_id, &bindings)
        .await
        .expect("all created accounts should resolve by IBAN");
    assert!(
        accounts
            .iter()
            .any(|account| account.monetary_account_id == source_id)
    );
    assert!(
        accounts
            .iter()
            .any(|account| account.monetary_account_id == created_bank)
    );
    assert!(
        accounts
            .iter()
            .any(|account| account.monetary_account_id == created_savings)
    );

    let initial = read_balances(&client, &accounts)
        .await
        .expect("all account balances should be readable");
    let evaluate = clepsydra_eval!(
        r#"
        [nodes.source]
        [nodes.created-bank]
        [nodes.created-savings]

        [[flow]]
        source = "source"
        target = "created-savings"
        volume = 50
        "#
    );
    let state = [
        ("source", balance_for(&initial, "source")),
        ("created-bank", balance_for(&initial, "created-bank")),
        ("created-savings", balance_for(&initial, "created-savings")),
    ];
    let target = evaluate(state).expect("valid Clepsydra state");
    let settlement =
        settle(&initial, &target, &accounts).expect("created-account graph should balance");
    assert_eq!(settlement.len(), 1);
    assert_eq!(settlement[0].source.name, "source");
    assert_eq!(settlement[0].target.name, "created-savings");
    assert_eq!(settlement[0].amount_minor, 50);

    if std::env::var("SANDBOX_PAYMENTS").as_deref() == Ok("1") {
        execute(&client, &settlement[0])
            .await
            .expect("same-user sandbox payment should succeed");
        let mut arrived = false;
        for _ in 0..10 {
            let current = read_balances(&client, &accounts)
                .await
                .expect("post-payment balances should be readable");
            if balance_for(&current, "created-savings")
                >= balance_for(&initial, "created-savings") + 50
            {
                arrived = true;
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        assert!(
            arrived,
            "same-user payment did not arrive in savings account"
        );
    }
}

fn balance_for(balances: &[(&str, u64)], name: &str) -> u64 {
    balances
        .iter()
        .find(|balance| balance.0 == name)
        .map_or(0, |balance| balance.1)
}

#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
async fn provision_funded_account() -> (Client, u64, u64, String) {
    let api_key = create_sandbox_user(SandboxUserKind::Person)
        .await
        .expect("sandbox user creation should succeed");
    let (context, session) = install_device(
        api_key,
        "https://public-api.sandbox.bunq.com/v1",
        "bunq-graph-sandbox-test/0.1",
        "bunq-graph multi-account sandbox test",
    )
    .await
    .expect("sandbox installation should succeed");
    let client =
        Client::from_installation(&context, session.token()).expect("sandbox client should build");
    let bank_accounts = client
        .list_monetary_account_banks(session.user_id())
        .await
        .expect("bank accounts should be readable");
    let bank = bank_accounts
        .response
        .first()
        .expect("sandbox user should have a bank account");
    let iban = find_iban(&bank.additional).expect("sandbox bank should expose an IBAN");
    let funding = RequestInquiryRequest::new(
        "1.00",
        "EUR",
        CounterpartyAlias::Email,
        "sugardaddy@bunq.com",
        Some("bunq-graph multi-account funding".to_owned()),
    )
    .expect("funding request should validate");
    client
        .create_request_inquiry(session.user_id(), bank.id, &funding)
        .await
        .expect("sandbox funding request should succeed");

    for _ in 0..10 {
        let refreshed = client
            .get_monetary_account_bank(session.user_id(), bank.id)
            .await
            .expect("bank balance should be readable");
        if refreshed.response[0]
            .balance
            .as_ref()
            .and_then(|balance| balance.value.parse::<f64>().ok())
            .is_some_and(|value| value >= 1.0)
        {
            return (client, session.user_id(), bank.id, iban);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("sandbox funding did not arrive");
}

fn find_iban(value: &std::collections::HashMap<String, Value>) -> Option<String> {
    value.values().find_map(find_iban_value)
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
