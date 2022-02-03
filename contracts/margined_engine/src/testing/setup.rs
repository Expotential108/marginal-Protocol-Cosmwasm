use crate::{
    contract::{instantiate, execute, query, reply},
};
use cw20::{Cw20Coin};
use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor};
use cosmwasm_std::{Addr, Empty, Uint128};
use margined_perp::margined_engine::{
    InstantiateMsg,
};
use margined_perp::margined_vamm::{
    InstantiateMsg as VammInstantiateMsg,
};

pub struct ContractInfo {
    pub addr: Addr,
    pub id: u64,
}

pub struct TestingEnv {
    pub router: App,
    pub owner: Addr,
    pub alice: Addr,
    pub bob: Addr,
    pub usdc: ContractInfo,
    pub vamm: ContractInfo,
    pub engine: ContractInfo,
}

pub const DECIMAL_MULTIPLIER: Uint128 = Uint128::new(1_000_000_000u128);

fn contract_cw20() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new_with_empty(
        cw20_base::contract::execute,
        cw20_base::contract::instantiate,
        cw20_base::contract::query,
    );
    Box::new(contract)
}

fn contract_vamm() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new_with_empty(
        margined_vamm::contract::execute,
        margined_vamm::contract::instantiate,
        margined_vamm::contract::query,
    );
    Box::new(contract)
}

fn contract_engine() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new_with_empty(
        execute,
        instantiate,
        query,
    ).with_reply(reply);
    Box::new(contract)
}

fn mock_app() -> App {
    AppBuilder::new().build()
}

pub fn setup() -> TestingEnv {
    let mut router = mock_app();

    let owner = Addr::unchecked("owner");
    let alice = Addr::unchecked("alice");
    let bob = Addr::unchecked("bob");

    let usdc_id = router.store_code(contract_cw20());
    let engine_id = router.store_code(contract_engine());
    let vamm_id = router.store_code(contract_vamm());

    let usdc_addr = router.instantiate_contract(
        usdc_id,
        owner.clone(),
        & cw20_base::msg::InstantiateMsg {
            name: "USDC".to_string(),
            symbol: "USDC".to_string(),
            decimals: 9,
            initial_balances: vec![
                Cw20Coin {
                    address: alice.to_string(),
                    amount: Uint128::new(5000) * DECIMAL_MULTIPLIER, // this is 5000 with 9dp
                },
                Cw20Coin {
                    address: bob.to_string(),
                    amount: Uint128::new(5000) * DECIMAL_MULTIPLIER, // this is 5000 with 9dp
                }
            ],
            mint: None,
            marketing: None,
        },
        &[],
        "cw20",
        None
    ).unwrap();

    let vamm_addr = router.instantiate_contract(
        vamm_id,
        owner.clone(),
        &VammInstantiateMsg {
            decimals: 9u8,
            quote_asset: "ETH".to_string(),
            base_asset: "USD".to_string(),
            quote_asset_reserve: Uint128::new(1_000) * DECIMAL_MULTIPLIER,
            base_asset_reserve: Uint128::new(100) * DECIMAL_MULTIPLIER,
            funding_period: 3_600 as u64,
        },
        &[],
        "vamm",
        None
    ).unwrap();

    // set up margined engine contract    
    let engine_addr = router
        .instantiate_contract(
            engine_id,
            owner.clone(),
            &InstantiateMsg {
                decimals: 9u8,
                eligible_collateral: usdc_addr.to_string(),
                initial_margin_ratio: Uint128::from(100u128), 
                maintenance_margin_ratio: Uint128::from(100u128), 
                liquidation_fee: Uint128::from(100u128),
            },
            &[],
            "engine",
            None,
        )
        .unwrap();

    TestingEnv {
        router,
        owner,
        alice,
        bob,
        usdc: ContractInfo {
            addr: usdc_addr,
            id: usdc_id,
        },
        vamm: ContractInfo {
            addr: vamm_addr,
            id: vamm_id,
        },
        engine: ContractInfo {
            addr: engine_addr,
            id: engine_id,
        },
    }
}