// Copyright © Aptos Foundation

// Copyright (c) Aptos
// SPDX-License-Identifier: Apache-2.0

// This is required because a diesel macro makes clippy sad
#![allow(clippy::extra_unused_lifetimes)]
#![allow(clippy::unused_unit)]

use super::signatures::Signature;
use crate::{
    schema::user_transactions,
    utils::util::{
        get_entry_function_contract_address_from_user_request,
        get_entry_function_from_user_request, get_entry_function_function_name_from_user_request,
        get_entry_function_module_name_from_user_request, parse_timestamp, standardize_address,
        u64_to_bigdecimal,
    },
};
use aptos_protos::{
    transaction::v1::{UserTransaction as UserTransactionPB, UserTransactionRequest},
    util::timestamp::Timestamp,
};
use bigdecimal::BigDecimal;
use field_count::FieldCount;
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Debug, FieldCount, Identifiable, Insertable, Serialize)]
#[diesel(primary_key(version))]
#[diesel(table_name = user_transactions)]
pub struct UserTransaction {
    pub version: i64,
    pub block_height: i64,
    pub parent_signature_type: String,
    pub sender: String,
    pub sequence_number: i64,
    pub max_gas_amount: BigDecimal,
    pub expiration_timestamp_secs: chrono::NaiveDateTime,
    pub gas_unit_price: BigDecimal,
    pub timestamp: chrono::NaiveDateTime,
    pub entry_function_id_str: String,
    pub epoch: i64,
    pub entry_function_contract_address: Option<String>,
    pub entry_function_module_name: Option<String>,
    pub entry_function_function_name: Option<String>,
}

impl UserTransaction {
    pub fn from_transaction(
        txn: &UserTransactionPB,
        timestamp: &Timestamp,
        block_height: i64,
        epoch: i64,
        version: i64,
    ) -> (Self, Vec<Signature>) {
        let user_request = txn
            .request
            .as_ref()
            .expect("Sends is not present in user txn");
        (
            Self {
                version,
                block_height,
                parent_signature_type: txn
                    .request
                    .as_ref()
                    .unwrap()
                    .signature
                    .as_ref()
                    .map(|sig| Signature::get_signature_type(sig, version))
                    .unwrap_or_default(),
                sender: standardize_address(&user_request.sender),
                sequence_number: user_request.sequence_number as i64,
                max_gas_amount: u64_to_bigdecimal(user_request.max_gas_amount),
                expiration_timestamp_secs: parse_timestamp(
                    user_request
                        .expiration_timestamp_secs
                        .as_ref()
                        .expect("Expiration timestamp is not present in user txn"),
                    version,
                ),
                gas_unit_price: u64_to_bigdecimal(user_request.gas_unit_price),
                timestamp: parse_timestamp(timestamp, version),
                entry_function_id_str: get_entry_function_from_user_request(user_request)
                    .unwrap_or_default(),
                epoch,
                entry_function_contract_address: Some(
                    get_entry_function_contract_address_from_user_request(user_request)
                        .unwrap_or_default(),
                ),
                entry_function_module_name: Some(
                    get_entry_function_module_name_from_user_request(user_request)
                        .unwrap_or_default(),
                ),
                entry_function_function_name: Some(
                    get_entry_function_function_name_from_user_request(user_request)
                        .unwrap_or_default(),
                ),
            },
            Self::get_signatures(user_request, version, block_height),
        )
    }

    /// Empty vec if signature is None
    pub fn get_signatures(
        user_request: &UserTransactionRequest,
        version: i64,
        block_height: i64,
    ) -> Vec<Signature> {
        user_request
            .signature
            .as_ref()
            .map(|s| {
                Signature::from_user_transaction(s, &user_request.sender, version, block_height)
                    .unwrap()
            })
            .unwrap_or_default()
    }
}

// Prevent conflicts with other things named `Transaction`
pub type UserTransactionModel = UserTransaction;
