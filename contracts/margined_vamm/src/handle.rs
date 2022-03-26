use cosmwasm_std::{
    Deps, DepsMut, Env, MessageInfo, Response, StdError, StdResult, Storage, Uint128,
};

use margined_common::integer::Integer;
use margined_perp::margined_vamm::Direction;

use crate::{
    contract::{ONE_DAY_IN_SECONDS, ONE_HOUR_IN_SECONDS},
    querier::query_underlying_twap_price,
    query::query_twap_price,
    state::{read_config, read_state, store_config, store_state, Config, State},
    utils::{
        add_reserve_snapshot, check_is_over_block_fluctuation_limit, modulo, require_margin_engine,
        require_open,
    },
};

#[allow(clippy::too_many_arguments)]
pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    toll_ratio: Option<Uint128>,
    spread_ratio: Option<Uint128>,
    fluctuation_limit_ratio: Option<Uint128>,
    margin_engine: Option<String>,
    pricefeed: Option<String>,
    spot_price_twap_interval: Option<u64>,
) -> StdResult<Response> {
    let mut config: Config = read_config(deps.storage)?;

    // check permission
    if info.sender != config.owner {
        return Err(StdError::generic_err("unauthorized"));
    }

    // change owner of amm
    if let Some(owner) = owner {
        config.owner = deps.api.addr_validate(owner.as_str())?;
    }

    // set and update margin engine
    if let Some(margin_engine) = margin_engine {
        config.margin_engine = deps.api.addr_validate(margin_engine.as_str())?;
    }

    // change toll ratio
    if let Some(toll_ratio) = toll_ratio {
        config.toll_ratio = toll_ratio;
    }

    // change spread ratio
    if let Some(spread_ratio) = spread_ratio {
        config.spread_ratio = spread_ratio;
    }

    // change fluctuation limit ratio
    if let Some(fluctuation_limit_ratio) = fluctuation_limit_ratio {
        config.fluctuation_limit_ratio = fluctuation_limit_ratio;
    }

    // change pricefeed
    if let Some(pricefeed) = pricefeed {
        config.pricefeed = deps.api.addr_validate(&pricefeed).unwrap();
    }

    // change spot price twap interval
    if let Some(spot_price_twap_interval) = spot_price_twap_interval {
        config.spot_price_twap_interval = spot_price_twap_interval;
    }

    store_config(deps.storage, &config)?;

    Ok(Response::default())
}

pub fn set_open(deps: DepsMut, env: Env, info: MessageInfo, open: bool) -> StdResult<Response> {
    let config: Config = read_config(deps.storage)?;
    let mut state: State = read_state(deps.storage)?;

    // check permission and if state matches
    if info.sender != config.owner || state.open == open {
        return Err(StdError::generic_err("unauthorized"));
    }

    state.open = open;

    // if state.open is true then we update the next funding time
    if state.open {
        state.next_funding_time = env.block.time.seconds()
            + config.funding_period / ONE_HOUR_IN_SECONDS * ONE_HOUR_IN_SECONDS;
    }

    store_state(deps.storage, &state)?;

    Ok(Response::default())
}

// Function should only be called by the margin engine
pub fn swap_input(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    direction: Direction,
    quote_asset_amount: Uint128,
    can_go_over_fluctuation: bool,
) -> StdResult<Response> {
    let state: State = read_state(deps.storage)?;
    let config: Config = read_config(deps.storage)?;

    require_open(state.open)?;
    require_margin_engine(info.sender, config.margin_engine)?;

    let base_asset_amount = get_input_price_with_reserves(
        deps.as_ref(),
        &direction,
        quote_asset_amount,
        state.quote_asset_reserve,
        state.base_asset_reserve,
    )?;

    update_reserve(
        deps.storage,
        env,
        direction.clone(),
        quote_asset_amount,
        base_asset_amount,
        can_go_over_fluctuation,
    )?;

    Ok(Response::new().add_attributes(vec![
        ("action", "swap_input"),
        ("direction", &direction.to_string()),
        ("quote_asset_amount", &quote_asset_amount.to_string()),
        ("base_asset_amount", &base_asset_amount.to_string()),
    ]))
}

// Function should only be called by the margin engine
pub fn swap_output(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    direction: Direction,
    base_asset_amount: Uint128,
) -> StdResult<Response> {
    let state: State = read_state(deps.storage)?;
    let config: Config = read_config(deps.storage)?;

    require_open(state.open)?;
    require_margin_engine(info.sender, config.margin_engine)?;

    let quote_asset_amount = get_output_price_with_reserves(
        deps.as_ref(),
        &direction,
        base_asset_amount,
        state.quote_asset_reserve,
        state.base_asset_reserve,
    )?;

    // flip direction when updating reserve
    let mut update_direction = direction.clone();
    if update_direction == Direction::AddToAmm {
        update_direction = Direction::RemoveFromAmm;
    } else {
        update_direction = Direction::AddToAmm;
    }

    update_reserve(
        deps.storage,
        env,
        update_direction,
        quote_asset_amount,
        base_asset_amount,
        true,
    )?;

    Ok(Response::new().add_attributes(vec![
        ("action", "swap_output"),
        ("direction", &direction.to_string()),
        ("quote_asset_amount", &quote_asset_amount.to_string()),
        ("base_asset_amount", &base_asset_amount.to_string()),
    ]))
}

pub fn settle_funding(deps: DepsMut, env: Env, info: MessageInfo) -> StdResult<Response> {
    let config: Config = read_config(deps.storage)?;
    let mut state: State = read_state(deps.storage)?;

    require_open(state.open)?;
    require_margin_engine(info.sender, config.margin_engine)?;

    if env.block.time.seconds() < state.next_funding_time {
        return Err(StdError::generic_err("settle funding called too early"));
    }

    // twap price from oracle
    let underlying_price: Uint128 =
        query_underlying_twap_price(&deps.as_ref(), config.spot_price_twap_interval)?;

    // twap price from here, i.e. the amm
    let index_price: Uint128 =
        query_twap_price(deps.as_ref(), env.clone(), config.spot_price_twap_interval)?;

    // let premium = calculate_premium(underlying_price, index_price)?;
    let premium = Integer::new_positive(index_price) - Integer::new_positive(underlying_price);

    let premium_fraction = premium * Integer::new_positive(config.funding_period)
        / Integer::new_positive(ONE_DAY_IN_SECONDS);

    // update funding rate = premiumFraction / twapIndexPrice
    state.funding_rate = premium_fraction.value.checked_div(underlying_price)?;

    // in order to prevent multiple funding settlement during very short time after network congestion
    let min_next_funding_time = env.block.time.plus_seconds(config.funding_buffer_period);

    // floor((nextFundingTime + fundingPeriod) / 3600) * 3600
    let next_funding_time = (env.block.time.seconds() + config.funding_period)
        / ONE_HOUR_IN_SECONDS
        * ONE_HOUR_IN_SECONDS;

    // max(nextFundingTimeOnHourStart, minNextValidFundingTime)
    state.next_funding_time = if next_funding_time > min_next_funding_time.seconds() {
        next_funding_time
    } else {
        min_next_funding_time.seconds()
    };

    store_state(deps.storage, &state)?;

    Ok(Response::new().add_attributes(vec![
        ("action", "settle_funding"),
        ("premium_fraction", &premium_fraction.to_string()),
    ]))
}

pub fn get_input_price_with_reserves(
    deps: Deps,
    direction: &Direction,
    quote_asset_amount: Uint128,
    quote_asset_reserve: Uint128,
    base_asset_reserve: Uint128,
) -> StdResult<Uint128> {
    let config: Config = read_config(deps.storage)?;

    if quote_asset_amount == Uint128::zero() {
        Uint128::zero();
    }

    // k = x * y (divided by decimal places)
    let invariant_k = quote_asset_reserve
        .checked_mul(base_asset_reserve)?
        .checked_div(config.decimals)?;

    let quote_asset_after: Uint128 = match direction {
        Direction::AddToAmm => quote_asset_reserve.checked_add(quote_asset_amount)?,
        Direction::RemoveFromAmm => quote_asset_reserve.checked_sub(quote_asset_amount)?,
    };

    let base_asset_after: Uint128 = invariant_k
        .checked_mul(config.decimals)?
        .checked_div(quote_asset_after)?;

    let mut base_asset_bought = if base_asset_after > base_asset_reserve {
        base_asset_after - base_asset_reserve
    } else {
        base_asset_reserve - base_asset_after
    };

    let remainder = modulo(invariant_k, quote_asset_after);
    if remainder != Uint128::zero() {
        if *direction == Direction::AddToAmm {
            base_asset_bought = base_asset_bought.checked_sub(Uint128::new(1u128))?;
        } else {
            base_asset_bought = base_asset_bought.checked_add(Uint128::from(1u128))?;
        }
    }

    Ok(base_asset_bought)
}

pub fn get_output_price_with_reserves(
    deps: Deps,
    direction: &Direction,
    base_asset_amount: Uint128,
    quote_asset_reserve: Uint128,
    base_asset_reserve: Uint128,
) -> StdResult<Uint128> {
    let config: Config = read_config(deps.storage)?;

    if base_asset_amount == Uint128::zero() {
        Uint128::zero();
    }
    let invariant_k = quote_asset_reserve
        .checked_mul(base_asset_reserve)?
        .checked_div(config.decimals)?;

    let base_asset_after: Uint128 = match direction {
        Direction::AddToAmm => base_asset_reserve.checked_add(base_asset_amount)?,
        Direction::RemoveFromAmm => base_asset_reserve.checked_sub(base_asset_amount)?,
    };

    let quote_asset_after: Uint128 = invariant_k
        .checked_mul(config.decimals)?
        .checked_div(base_asset_after)?;

    let mut quote_asset_sold = if quote_asset_after > quote_asset_reserve {
        quote_asset_after - quote_asset_reserve
    } else {
        quote_asset_reserve - quote_asset_after
    };

    let remainder = modulo(invariant_k, base_asset_after);
    if remainder != Uint128::zero() {
        if *direction == Direction::AddToAmm {
            quote_asset_sold = quote_asset_sold.checked_sub(Uint128::from(1u128))?;
        } else {
            quote_asset_sold = quote_asset_sold.checked_add(Uint128::new(1u128))?;
        }
    }
    Ok(quote_asset_sold)
}

pub fn update_reserve(
    storage: &mut dyn Storage,
    env: Env,
    direction: Direction,
    quote_asset_amount: Uint128,
    base_asset_amount: Uint128,
    can_go_over_fluctuation: bool,
) -> StdResult<Response> {
    let state: State = read_state(storage)?;
    let mut update_state = state.clone();

    check_is_over_block_fluctuation_limit(
        storage,
        env.clone(),
        direction.clone(),
        quote_asset_amount,
        base_asset_amount,
        can_go_over_fluctuation,
    )?;

    match direction {
        Direction::AddToAmm => {
            update_state.quote_asset_reserve = update_state
                .quote_asset_reserve
                .checked_add(quote_asset_amount)?;
            update_state.base_asset_reserve =
                state.base_asset_reserve.checked_sub(base_asset_amount)?;

            // TODO think whether this needs overflow protection
            update_state.total_position_size =
                state.total_position_size + Integer::from(base_asset_amount);
        }
        Direction::RemoveFromAmm => {
            update_state.base_asset_reserve = update_state
                .base_asset_reserve
                .checked_add(base_asset_amount)?;
            update_state.quote_asset_reserve =
                state.quote_asset_reserve.checked_sub(quote_asset_amount)?;

            // TODO think whether this needs underflow protection
            update_state.total_position_size =
                state.total_position_size - Integer::from(base_asset_amount);
        }
    }

    store_state(storage, &update_state)?;

    add_reserve_snapshot(
        storage,
        env,
        update_state.quote_asset_reserve,
        update_state.base_asset_reserve,
    )?;

    Ok(Response::new())
}
