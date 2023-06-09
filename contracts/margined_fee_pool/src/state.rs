use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use cosmwasm_std::{Deps, DepsMut, StdError::GenericErr, StdResult, Storage};
use cosmwasm_storage::singleton;
use cw_storage_plus::Item;
use margined_common::asset::AssetInfo;

pub static KEY_CONFIG: &[u8] = b"config";
pub const TOKEN_LIST: Item<Vec<AssetInfo>> = Item::new("token-list");
pub const TOKEN_LIMIT: usize = 3usize;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, JsonSchema)]
pub struct Config {}

pub fn store_config(storage: &mut dyn Storage, config: &Config) -> StdResult<()> {
    singleton(storage, KEY_CONFIG).save(config)
}

// function checks if an addr is already added and adds it if not
// We also check that we have not reached the limit of tokens here
pub fn save_token(deps: DepsMut, input: AssetInfo) -> StdResult<()> {
    // check if the list exists already
    let mut token_list = match TOKEN_LIST.may_load(deps.storage)? {
        None => vec![],
        Some(list) => list,
    };

    // check if we already added the token
    if token_list.contains(&input) {
        return Err(GenericErr {
            msg: "This token is already added".to_string(),
        });
    };

    // check if we have reached the capacity
    if token_list.len() >= TOKEN_LIMIT {
        return Err(GenericErr {
            msg: "The token capacity is already reached".to_string(),
        });
    };

    // add the token
    token_list.push(input);
    TOKEN_LIST.save(deps.storage, &token_list)
}

// this function reads Addrs stored in the TOKEN_LIST.
// note that this function ONLY takes the first TOKEN_LIMIT terms
pub fn read_token_list(deps: Deps, limit: usize) -> StdResult<Vec<AssetInfo>> {
    match TOKEN_LIST.may_load(deps.storage)? {
        None => Err(GenericErr {
            msg: "No tokens are stored".to_string(),
        }),
        Some(list) => {
            let take = limit.min(list.len());
            Ok(list[..take].to_vec())
        }
    }
}

// this function checks whether the token is stored already
pub fn is_token(storage: &dyn Storage, token: AssetInfo) -> bool {
    match TOKEN_LIST.may_load(storage).unwrap() {
        None => false,
        Some(list) => list.contains(&token),
    }
}

// this function deletes the entry under the given key
pub fn remove_token(deps: DepsMut, token: AssetInfo) -> StdResult<()> {
    // check if the list exists
    let mut token_list = match TOKEN_LIST.may_load(deps.storage)? {
        None => {
            return Err(GenericErr {
                msg: "No tokens are stored".to_string(),
            })
        }
        Some(value) => value,
    };

    // check if the token is added
    if !token_list.contains(&token) {
        return Err(GenericErr {
            msg: "This token has not been added".to_string(),
        });
    }

    // change token_list
    let index = token_list
        .clone()
        .iter()
        .position(|x| x.eq(&token))
        .unwrap();
    token_list.swap_remove(index);

    // saves the updated token_list
    TOKEN_LIST.save(deps.storage, &token_list)
}
