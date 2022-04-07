// use crate::testing::setup::{self, to_decimals};
use cosmwasm_std::{Coin, Uint128};
use cw20::Cw20ExecuteMsg;
use cw_multi_test::Executor;
use margined_common::integer::Integer;
use margined_perp::margined_engine::{PnlCalcOption, PositionResponse, Side};
use margined_utils::scenarios::NativeTokenScenario;

// Note: these tests also verify the 10% fees for the amm are functioning

#[test]
fn test_initialization() {
    let NativeTokenScenario {
        router,
        owner,
        alice,
        bob,
        engine,
        ..
    } = NativeTokenScenario::new();

    // verfiy the balances
    let owner_balance = router.wrap().query_balance(&owner, "uusd").unwrap().amount;
    assert_eq!(owner_balance, Uint128::zero());
    let alice_balance = router.wrap().query_balance(&alice, "uusd").unwrap().amount;
    assert_eq!(alice_balance, Uint128::new(5_000_000_000));
    let bob_balance = router.wrap().query_balance(&bob, "uusd").unwrap().amount;
    assert_eq!(bob_balance, Uint128::new(5_000_000_000));
    let engine_balance = router.wrap().query_balance(&engine.addr(), "uusd").unwrap().amount;
    assert_eq!(engine_balance, Uint128::zero());
}

#[test]
fn test_force_error_open_position_no_token_sent() {
    let NativeTokenScenario {
        mut router,
        owner,
        alice,
        bob,
        engine,
        vamm,
        ..
    } = NativeTokenScenario::new();

    // 10% fee
    let msg = vamm.set_toll_ratio(Uint128::from(100_000u128)).unwrap();
    router.execute(owner.clone(), msg).unwrap();

    let msg =  vamm.set_spread_ratio(Uint128::zero()).unwrap();
    router.execute(owner.clone(), msg).unwrap();


    let msg = engine
        .open_position(
            vamm.addr().to_string(),
            Side::BUY,
            Uint128::from(60_000_000u64),
            Uint128::from(10_000_000u64),
            Uint128::from(37_500_000u64),
            vec![],
        )
        .unwrap();
    let response = router.execute(alice.clone(), msg).unwrap_err();

    assert_eq!(response.to_string(), "Generic error: Native token balance mismatch between the argument and the transferred".to_string());
}


#[test]
fn test_ten_percent_fee_open_long_position() {
    let NativeTokenScenario {
        mut router,
        owner,
        alice,
        bob,
        engine,
        vamm,
        ..
    } = NativeTokenScenario::new();

    // 10% fee
    let msg = vamm.set_toll_ratio(Uint128::from(100_000u128)).unwrap();
    router.execute(owner.clone(), msg).unwrap();

    let msg =  vamm.set_spread_ratio(Uint128::zero()).unwrap();
    router.execute(owner.clone(), msg).unwrap();


    let msg = engine
        .open_position(
            vamm.addr().to_string(),
            Side::BUY,
            Uint128::from(60_000_000u64),
            Uint128::from(10_000_000u64),
            Uint128::from(37_500_000u64),
            vec![Coin::new(60_000_000u128, "uusd")],
        )
        .unwrap();
    router.execute(alice.clone(), msg).unwrap();

    // expect to be 60
    let margin = engine
        .get_balance_with_funding_payment(&router, alice.to_string())
        .unwrap();
    assert_eq!(margin, Uint128::from(60_000_000u64));

    // personal position should be 37.5
    let position: PositionResponse = engine
        .position(&router, vamm.addr().to_string(), alice.to_string())
        .unwrap();
    assert_eq!(position.size, Integer::new_positive(37_500_000u128));
    assert_eq!(position.margin, Uint128::from(60_000_000u64));
}
