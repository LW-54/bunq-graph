use bunq_graph::{Account, AccountKind, settle};
use clepsydra_lib::clepsydra_eval;

#[test]
fn evaluates_clepsydra_then_minimizes_bunq_transfers() {
    let evaluate = clepsydra_eval!(
        r#"
        [nodes.sink]

        [nodes.checking]
        capacity = 100
        target = "sink"

        [[flow]]
        source = "checking"
        target = "sink"
        volume = 30
        "#
    );
    let initial = [("checking", 80), ("sink", 20)];
    let target = evaluate(initial).expect("valid Clepsydra state");
    let accounts = [
        Account {
            name: "checking".to_owned(),
            iban: "NL00checking".to_owned(),
            kind: AccountKind::Bank,
            user_id: 1,
            monetary_account_id: 1,
            currency: "EUR".to_owned(),
        },
        Account {
            name: "sink".to_owned(),
            iban: "NL00sink".to_owned(),
            kind: AccountKind::Bank,
            user_id: 1,
            monetary_account_id: 2,
            currency: "EUR".to_owned(),
        },
    ];
    let settlement =
        settle(&initial, &target, &accounts).expect("Clepsydra preserves total volume");

    assert_eq!(target[0].1, 50);
    assert_eq!(target[1].1, 50);
    assert_eq!(settlement.len(), 1);
    assert_eq!(settlement[0].amount_minor, 30);
}
