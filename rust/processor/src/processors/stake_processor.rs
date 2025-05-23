// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use super::{DefaultProcessingResult, ProcessorName, ProcessorTrait};
use crate::{
    db::{
        common::models::stake_models::{
            current_delegated_voter::CurrentDelegatedVoter,
            delegator_activities::{
                RawDelegatedStakingActivity, RawDelegatedStakingActivityConvertible,
            },
            delegator_balances::{
                RawCurrentDelegatorBalance, RawCurrentDelegatorBalanceConvertible,
                RawCurrentDelegatorBalanceMap, RawDelegatorBalance, RawDelegatorBalanceConvertible,
            },
            delegator_pools::{
                DelegatorPool, DelegatorPoolMap, RawCurrentDelegatorPoolBalance,
                RawCurrentDelegatorPoolBalanceConvertible, RawDelegatorPoolBalance,
                RawDelegatorPoolBalanceConvertible,
            },
            proposal_voters::{RawProposalVote, RawProposalVoteConvertible},
            stake_utils::DelegationVoteGovernanceRecordsResource,
            staking_pool_voter::{
                RawCurrentStakingPoolVoter, RawCurrentStakingPoolVoterConvertible,
                StakingPoolRawVoterMap,
            },
        },
        postgres::models::stake_models::{
            delegator_activities::DelegatedStakingActivity,
            delegator_balances::{CurrentDelegatorBalance, DelegatorBalance},
            delegator_pools::{CurrentDelegatorPoolBalance, DelegatorPoolBalance},
            proposal_votes::ProposalVote,
            staking_pool_voter::CurrentStakingPoolVoter,
        },
    },
    gap_detectors::ProcessingResult,
    schema,
    utils::{
        database::{execute_in_chunks, get_config_table_chunk_size, ArcDbPool, DbPoolConnection},
        util::{parse_timestamp, standardize_address},
    },
    IndexerGrpcProcessorConfig,
};
use ahash::AHashMap;
use anyhow::bail;
use aptos_protos::transaction::v1::{write_set_change::Change, Transaction};
use async_trait::async_trait;
use diesel::{
    pg::{upsert::excluded, Pg},
    query_builder::QueryFragment,
    ExpressionMethods,
};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use tracing::error;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StakeProcessorConfig {
    #[serde(default = "IndexerGrpcProcessorConfig::default_query_retries")]
    pub query_retries: u32,
    #[serde(default = "IndexerGrpcProcessorConfig::default_query_retry_delay_ms")]
    pub query_retry_delay_ms: u64,
}

pub struct StakeProcessor {
    connection_pool: ArcDbPool,
    config: StakeProcessorConfig,
    per_table_chunk_sizes: AHashMap<String, usize>,
}

impl StakeProcessor {
    pub fn new(
        connection_pool: ArcDbPool,
        config: StakeProcessorConfig,
        per_table_chunk_sizes: AHashMap<String, usize>,
    ) -> Self {
        Self {
            connection_pool,
            config,
            per_table_chunk_sizes,
        }
    }
}

impl Debug for StakeProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = &self.connection_pool.state();
        write!(
            f,
            "StakeTransactionProcessor {{ connections: {:?}  idle_connections: {:?} }}",
            state.connections, state.idle_connections
        )
    }
}

async fn insert_to_db(
    conn: ArcDbPool,
    name: &'static str,
    start_version: u64,
    end_version: u64,
    current_stake_pool_voters: &[CurrentStakingPoolVoter],
    proposal_votes: &[ProposalVote],
    delegator_actvities: &[DelegatedStakingActivity],
    delegator_balances: &[DelegatorBalance],
    current_delegator_balances: &[CurrentDelegatorBalance],
    delegator_pools: &[DelegatorPool],
    delegator_pool_balances: &[DelegatorPoolBalance],
    current_delegator_pool_balances: &[CurrentDelegatorPoolBalance],
    current_delegated_voter: &[CurrentDelegatedVoter],
    per_table_chunk_sizes: &AHashMap<String, usize>,
) -> Result<(), diesel::result::Error> {
    tracing::trace!(
        name = name,
        start_version = start_version,
        end_version = end_version,
        "Inserting to db",
    );

    let cspv = execute_in_chunks(
        conn.clone(),
        insert_current_stake_pool_voter_query,
        current_stake_pool_voters,
        get_config_table_chunk_size::<CurrentStakingPoolVoter>(
            "current_staking_pool_voter",
            per_table_chunk_sizes,
        ),
    );
    let pv = execute_in_chunks(
        conn.clone(),
        insert_proposal_votes_query,
        proposal_votes,
        get_config_table_chunk_size::<ProposalVote>("proposal_votes", per_table_chunk_sizes),
    );
    let da = execute_in_chunks(
        conn.clone(),
        insert_delegator_activities_query,
        delegator_actvities,
        get_config_table_chunk_size::<DelegatedStakingActivity>(
            "delegated_staking_activities",
            per_table_chunk_sizes,
        ),
    );
    let db = execute_in_chunks(
        conn.clone(),
        insert_delegator_balances_query,
        delegator_balances,
        get_config_table_chunk_size::<DelegatorBalance>(
            "delegator_balances",
            per_table_chunk_sizes,
        ),
    );
    let cdb = execute_in_chunks(
        conn.clone(),
        insert_current_delegator_balances_query,
        current_delegator_balances,
        get_config_table_chunk_size::<CurrentDelegatorBalance>(
            "current_delegator_balances",
            per_table_chunk_sizes,
        ),
    );
    let dp = execute_in_chunks(
        conn.clone(),
        insert_delegator_pools_query,
        delegator_pools,
        get_config_table_chunk_size::<DelegatorPool>(
            "delegated_staking_pools",
            per_table_chunk_sizes,
        ),
    );
    let dpb = execute_in_chunks(
        conn.clone(),
        insert_delegator_pool_balances_query,
        delegator_pool_balances,
        get_config_table_chunk_size::<DelegatorPoolBalance>(
            "delegated_staking_pool_balances",
            per_table_chunk_sizes,
        ),
    );
    let cdpb = execute_in_chunks(
        conn.clone(),
        insert_current_delegator_pool_balances_query,
        current_delegator_pool_balances,
        get_config_table_chunk_size::<CurrentDelegatorPoolBalance>(
            "current_delegated_staking_pool_balances",
            per_table_chunk_sizes,
        ),
    );
    let cdv = execute_in_chunks(
        conn,
        insert_current_delegated_voter_query,
        current_delegated_voter,
        get_config_table_chunk_size::<CurrentDelegatedVoter>(
            "current_delegated_voter",
            per_table_chunk_sizes,
        ),
    );

    let (cspv_res, pv_res, da_res, db_res, cdb_res, dp_res, dpb_res, cdpb_res, cdv_res) =
        futures::join!(cspv, pv, da, db, cdb, dp, dpb, cdpb, cdv);
    for res in [
        cspv_res, pv_res, da_res, db_res, cdb_res, dp_res, dpb_res, cdpb_res, cdv_res,
    ] {
        res?;
    }

    Ok(())
}

pub fn insert_current_stake_pool_voter_query(
    items_to_insert: Vec<CurrentStakingPoolVoter>,
) -> (
    impl QueryFragment<Pg> + diesel::query_builder::QueryId + Send,
    Option<&'static str>,
) {
    use schema::current_staking_pool_voter::dsl::*;

    (diesel::insert_into(schema::current_staking_pool_voter::table)
         .values(items_to_insert)
         .on_conflict(staking_pool_address)
         .do_update()
         .set((
             staking_pool_address.eq(excluded(staking_pool_address)),
             voter_address.eq(excluded(voter_address)),
             last_transaction_version.eq(excluded(last_transaction_version)),
             inserted_at.eq(excluded(inserted_at)),
             operator_address.eq(excluded(operator_address)),
         )),
     Some(
         " WHERE current_staking_pool_voter.last_transaction_version <= EXCLUDED.last_transaction_version ",
     ),
    )
}

pub fn insert_proposal_votes_query(
    items_to_insert: Vec<ProposalVote>,
) -> (
    impl QueryFragment<Pg> + diesel::query_builder::QueryId + Send,
    Option<&'static str>,
) {
    use schema::proposal_votes::dsl::*;

    (
        diesel::insert_into(schema::proposal_votes::table)
            .values(items_to_insert)
            .on_conflict((transaction_version, proposal_id, voter_address))
            .do_nothing(),
        None,
    )
}

pub fn insert_delegator_activities_query(
    items_to_insert: Vec<DelegatedStakingActivity>,
) -> (
    impl QueryFragment<Pg> + diesel::query_builder::QueryId + Send,
    Option<&'static str>,
) {
    use schema::delegated_staking_activities::dsl::*;

    (
        diesel::insert_into(schema::delegated_staking_activities::table)
            .values(items_to_insert)
            .on_conflict((transaction_version, event_index))
            .do_nothing(),
        None,
    )
}

pub fn insert_delegator_balances_query(
    items_to_insert: Vec<DelegatorBalance>,
) -> (
    impl QueryFragment<Pg> + diesel::query_builder::QueryId + Send,
    Option<&'static str>,
) {
    use schema::delegator_balances::dsl::*;

    (
        diesel::insert_into(schema::delegator_balances::table)
            .values(items_to_insert)
            .on_conflict((transaction_version, write_set_change_index))
            .do_nothing(),
        None,
    )
}

pub fn insert_current_delegator_balances_query(
    items_to_insert: Vec<CurrentDelegatorBalance>,
) -> (
    impl QueryFragment<Pg> + diesel::query_builder::QueryId + Send,
    Option<&'static str>,
) {
    use schema::current_delegator_balances::dsl::*;

    (diesel::insert_into(schema::current_delegator_balances::table)
         .values(items_to_insert)
         .on_conflict((delegator_address, pool_address, pool_type, table_handle))
         .do_update()
         .set((
             last_transaction_version.eq(excluded(last_transaction_version)),
             inserted_at.eq(excluded(inserted_at)),
             shares.eq(excluded(shares)),
             parent_table_handle.eq(excluded(parent_table_handle)),
         )),
     Some(
         " WHERE current_delegator_balances.last_transaction_version <= EXCLUDED.last_transaction_version ",
     ),
    )
}

pub fn insert_delegator_pools_query(
    items_to_insert: Vec<DelegatorPool>,
) -> (
    impl QueryFragment<Pg> + diesel::query_builder::QueryId + Send,
    Option<&'static str>,
) {
    use schema::delegated_staking_pools::dsl::*;

    (diesel::insert_into(schema::delegated_staking_pools::table)
         .values(items_to_insert)
         .on_conflict(staking_pool_address)
         .do_update()
         .set((
             first_transaction_version.eq(excluded(first_transaction_version)),
             inserted_at.eq(excluded(inserted_at)),
         )),
     Some(
         " WHERE delegated_staking_pools.first_transaction_version >= EXCLUDED.first_transaction_version ",
     ),
    )
}

pub fn insert_delegator_pool_balances_query(
    items_to_insert: Vec<DelegatorPoolBalance>,
) -> (
    impl QueryFragment<Pg> + diesel::query_builder::QueryId + Send,
    Option<&'static str>,
) {
    use schema::delegated_staking_pool_balances::dsl::*;

    (
        diesel::insert_into(schema::delegated_staking_pool_balances::table)
            .values(items_to_insert)
            .on_conflict((transaction_version, staking_pool_address))
            .do_nothing(),
        None,
    )
}

pub fn insert_current_delegator_pool_balances_query(
    items_to_insert: Vec<CurrentDelegatorPoolBalance>,
) -> (
    impl QueryFragment<Pg> + diesel::query_builder::QueryId + Send,
    Option<&'static str>,
) {
    use schema::current_delegated_staking_pool_balances::dsl::*;

    (diesel::insert_into(schema::current_delegated_staking_pool_balances::table)
         .values(items_to_insert)
         .on_conflict(staking_pool_address)
         .do_update()
         .set((
             total_coins.eq(excluded(total_coins)),
             total_shares.eq(excluded(total_shares)),
             last_transaction_version.eq(excluded(last_transaction_version)),
             inserted_at.eq(excluded(inserted_at)),
             operator_commission_percentage.eq(excluded(operator_commission_percentage)),
             inactive_table_handle.eq(excluded(inactive_table_handle)),
             active_table_handle.eq(excluded(active_table_handle)),
         )),
     Some(
         " WHERE current_delegated_staking_pool_balances.last_transaction_version <= EXCLUDED.last_transaction_version ",
     ),
    )
}

pub fn insert_current_delegated_voter_query(
    item_to_insert: Vec<CurrentDelegatedVoter>,
) -> (
    impl QueryFragment<Pg> + diesel::query_builder::QueryId + Send,
    Option<&'static str>,
) {
    use schema::current_delegated_voter::dsl::*;

    (diesel::insert_into(schema::current_delegated_voter::table)
         .values(item_to_insert)
         .on_conflict((delegation_pool_address, delegator_address))
         .do_update()
         .set((
             voter.eq(excluded(voter)),
             pending_voter.eq(excluded(pending_voter)),
             last_transaction_timestamp.eq(excluded(last_transaction_timestamp)),
             last_transaction_version.eq(excluded(last_transaction_version)),
             table_handle.eq(excluded(table_handle)),
             inserted_at.eq(excluded(inserted_at)),
         )),
     Some(
         " WHERE current_delegated_voter.last_transaction_version <= EXCLUDED.last_transaction_version ",
     ),
    )
}

pub async fn parse_stake_data(
    transactions: &Vec<Transaction>,
    mut conn: Option<DbPoolConnection<'_>>,
    query_retries: u32,
    query_retry_delay_ms: u64,
) -> Result<
    (
        Vec<RawCurrentStakingPoolVoter>,
        Vec<RawProposalVote>,
        Vec<RawDelegatedStakingActivity>,
        Vec<RawDelegatorBalance>,
        Vec<RawCurrentDelegatorBalance>,
        Vec<DelegatorPool>,
        Vec<RawDelegatorPoolBalance>,
        Vec<RawCurrentDelegatorPoolBalance>,
        Vec<CurrentDelegatedVoter>,
    ),
    anyhow::Error,
> {
    let mut all_current_stake_pool_voters: StakingPoolRawVoterMap = AHashMap::new();
    let mut all_proposal_votes = vec![];
    let mut all_delegator_activities = vec![];
    let mut all_delegator_balances = vec![];
    let mut all_current_delegator_balances: RawCurrentDelegatorBalanceMap = AHashMap::new();
    let mut all_delegator_pools: DelegatorPoolMap = AHashMap::new();
    let mut all_delegator_pool_balances = vec![];
    let mut all_current_delegator_pool_balances = AHashMap::new();

    let mut active_pool_to_staking_pool = AHashMap::new();
    // structs needed to get delegated voters
    let mut all_current_delegated_voter = AHashMap::new();
    let mut all_vote_delegation_handle_to_pool_address = AHashMap::new();

    for txn in transactions {
        // Add votes data
        let current_stake_pool_voter = RawCurrentStakingPoolVoter::from_transaction(txn).unwrap();
        all_current_stake_pool_voters.extend(current_stake_pool_voter);
        let mut proposal_votes = RawProposalVote::from_transaction(txn).unwrap();
        all_proposal_votes.append(&mut proposal_votes);

        // Add delegator activities
        let mut delegator_activities = RawDelegatedStakingActivity::from_transaction(txn).unwrap();
        all_delegator_activities.append(&mut delegator_activities);

        // Add delegator pools
        let (delegator_pools, mut delegator_pool_balances, current_delegator_pool_balances) =
            DelegatorPool::from_transaction(txn).unwrap();
        all_delegator_pools.extend(delegator_pools);
        all_delegator_pool_balances.append(&mut delegator_pool_balances);
        all_current_delegator_pool_balances.extend(current_delegator_pool_balances);

        // Moving the transaction code here is the new paradigm to avoid redoing a lot of the duplicate work
        // Currently only delegator voting follows this paradigm
        // TODO: refactor all the other staking code to follow this paradigm
        let txn_version = txn.version as i64;
        let txn_timestamp = parse_timestamp(txn.timestamp.as_ref().unwrap(), txn_version);
        let transaction_info = txn.info.as_ref().expect("Transaction info doesn't exist!");
        // adding some metadata for subsequent parsing
        for wsc in &transaction_info.changes {
            if let Change::WriteResource(write_resource) = wsc.change.as_ref().unwrap() {
                if let Some(DelegationVoteGovernanceRecordsResource::GovernanceRecords(inner)) =
                    DelegationVoteGovernanceRecordsResource::from_write_resource(
                        write_resource,
                        txn_version,
                    )?
                {
                    let delegation_pool_address =
                        standardize_address(&write_resource.address.to_string());
                    let vote_delegation_handle = inner.vote_delegation.buckets.inner.get_handle();

                    all_vote_delegation_handle_to_pool_address
                        .insert(vote_delegation_handle, delegation_pool_address.clone());
                }
                if let Some(map) =
                    RawCurrentDelegatorBalance::get_active_pool_to_staking_pool_mapping(
                        write_resource,
                        txn_version,
                    )
                    .unwrap()
                {
                    active_pool_to_staking_pool.extend(map);
                }
            }
        }

        if let Some(ref mut conn) = conn {
            // Add delegator balances
            let (mut delegator_balances, current_delegator_balances) =
                RawCurrentDelegatorBalance::from_transaction(
                    txn,
                    &active_pool_to_staking_pool,
                    conn,
                    query_retries,
                    query_retry_delay_ms,
                )
                .await
                .unwrap();
            all_delegator_balances.append(&mut delegator_balances);
            all_current_delegator_balances.extend(current_delegator_balances);

            // this write table item indexing is to get delegator address, table handle, and voter & pending voter
            for wsc in &transaction_info.changes {
                if let Change::WriteTableItem(write_table_item) = wsc.change.as_ref().unwrap() {
                    let voter_map = CurrentDelegatedVoter::from_write_table_item(
                        write_table_item,
                        txn_version,
                        txn_timestamp,
                        &all_vote_delegation_handle_to_pool_address,
                        conn,
                        query_retries,
                        query_retry_delay_ms,
                    )
                    .await
                    .unwrap();

                    all_current_delegated_voter.extend(voter_map);
                }
            }

            // we need one last loop to prefill delegators that got in before the delegated voting contract was deployed
            for wsc in &transaction_info.changes {
                if let Change::WriteTableItem(write_table_item) = wsc.change.as_ref().unwrap() {
                    if let Some(voter) =
                        CurrentDelegatedVoter::get_delegators_pre_contract_deployment(
                            write_table_item,
                            txn_version,
                            txn_timestamp,
                            &active_pool_to_staking_pool,
                            &all_current_delegated_voter,
                            conn,
                            query_retries,
                            query_retry_delay_ms,
                        )
                        .await
                        .unwrap()
                    {
                        all_current_delegated_voter.insert(voter.pk(), voter);
                    }
                }
            }
        }
    }

    // Getting list of values and sorting by pk in order to avoid postgres deadlock since we're doing multi threaded db writes
    let mut all_current_stake_pool_voters = all_current_stake_pool_voters
        .into_values()
        .collect::<Vec<RawCurrentStakingPoolVoter>>();
    let mut all_current_delegator_balances = all_current_delegator_balances
        .into_values()
        .collect::<Vec<RawCurrentDelegatorBalance>>();
    let mut all_delegator_pools = all_delegator_pools
        .into_values()
        .collect::<Vec<DelegatorPool>>();
    let mut all_current_delegator_pool_balances = all_current_delegator_pool_balances
        .into_values()
        .collect::<Vec<RawCurrentDelegatorPoolBalance>>();
    let mut all_current_delegated_voter = all_current_delegated_voter
        .into_values()
        .collect::<Vec<CurrentDelegatedVoter>>();

    // Sort by PK
    all_current_stake_pool_voters
        .sort_by(|a, b| a.staking_pool_address.cmp(&b.staking_pool_address));
    all_current_delegator_balances.sort_by(|a, b| {
        (&a.delegator_address, &a.pool_address, &a.pool_type).cmp(&(
            &b.delegator_address,
            &b.pool_address,
            &b.pool_type,
        ))
    });

    all_delegator_pools.sort_by(|a, b| a.staking_pool_address.cmp(&b.staking_pool_address));
    all_current_delegator_pool_balances
        .sort_by(|a, b| a.staking_pool_address.cmp(&b.staking_pool_address));
    all_current_delegated_voter.sort();

    Ok((
        all_current_stake_pool_voters,
        all_proposal_votes,
        all_delegator_activities,
        all_delegator_balances,
        all_current_delegator_balances,
        all_delegator_pools,
        all_delegator_pool_balances,
        all_current_delegator_pool_balances,
        all_current_delegated_voter,
    ))
}

#[async_trait]
impl ProcessorTrait for StakeProcessor {
    fn name(&self) -> &'static str {
        ProcessorName::StakeProcessor.into()
    }

    async fn process_transactions(
        &self,
        transactions: Vec<Transaction>,
        start_version: u64,
        end_version: u64,
        _: Option<u64>,
    ) -> anyhow::Result<ProcessingResult> {
        let processing_start = std::time::Instant::now();
        let last_transaction_timestamp = transactions.last().unwrap().timestamp;

        let conn = self.get_conn().await;
        let query_retries = self.config.query_retries;
        let query_retry_delay_ms = self.config.query_retry_delay_ms;

        let (
            raw_all_current_stake_pool_voters,
            raw_all_proposal_votes,
            raw_all_delegator_activities,
            raw_all_delegator_balances,
            raw_all_current_delegator_balances,
            all_delegator_pools,
            raw_all_delegator_pool_balances,
            raw_all_current_delegator_pool_balances,
            all_current_delegated_voter,
        ) = match parse_stake_data(
            &transactions,
            Some(conn),
            query_retries,
            query_retry_delay_ms,
        )
        .await
        {
            Ok(data) => data,
            Err(e) => {
                error!(
                    start_version = start_version,
                    end_version = end_version,
                    processor_name = self.name(),
                    error = ?e,
                    "[Parser] Error parsing stake data",
                );
                bail!(e)
            },
        };
        let all_delegator_balances: Vec<DelegatorBalance> = raw_all_delegator_balances
            .into_iter()
            .map(DelegatorBalance::from_raw)
            .collect::<Vec<_>>();
        let all_current_delegator_balances = raw_all_current_delegator_balances
            .into_iter()
            .map(CurrentDelegatorBalance::from_raw)
            .collect::<Vec<_>>();
        let all_delegator_pool_balances = raw_all_delegator_pool_balances
            .into_iter()
            .map(DelegatorPoolBalance::from_raw)
            .collect::<Vec<_>>();
        let all_current_delegator_pool_balances = raw_all_current_delegator_pool_balances
            .into_iter()
            .map(CurrentDelegatorPoolBalance::from_raw)
            .collect::<Vec<_>>();
        let all_delegator_activities = raw_all_delegator_activities
            .into_iter()
            .map(DelegatedStakingActivity::from_raw)
            .collect::<Vec<_>>();
        let all_proposal_votes = raw_all_proposal_votes
            .into_iter()
            .map(ProposalVote::from_raw)
            .collect::<Vec<_>>();
        let all_current_stake_pool_voters = raw_all_current_stake_pool_voters
            .into_iter()
            .map(CurrentStakingPoolVoter::from_raw)
            .collect::<Vec<_>>();

        let processing_duration_in_secs = processing_start.elapsed().as_secs_f64();
        let db_insertion_start = std::time::Instant::now();

        let tx_result = insert_to_db(
            self.get_pool(),
            self.name(),
            start_version,
            end_version,
            &all_current_stake_pool_voters,
            &all_proposal_votes,
            &all_delegator_activities,
            &all_delegator_balances,
            &all_current_delegator_balances,
            &all_delegator_pools,
            &all_delegator_pool_balances,
            &all_current_delegator_pool_balances,
            &all_current_delegated_voter,
            &self.per_table_chunk_sizes,
        )
        .await;
        let db_insertion_duration_in_secs = db_insertion_start.elapsed().as_secs_f64();
        match tx_result {
            Ok(_) => Ok(ProcessingResult::DefaultProcessingResult(
                DefaultProcessingResult {
                    start_version,
                    end_version,
                    processing_duration_in_secs,
                    db_insertion_duration_in_secs,
                    last_transaction_timestamp,
                },
            )),
            Err(e) => {
                error!(
                    start_version = start_version,
                    end_version = end_version,
                    processor_name = self.name(),
                    error = ?e,
                    "[Parser] Error inserting transactions to db",
                );
                bail!(e)
            },
        }
    }

    fn connection_pool(&self) -> &ArcDbPool {
        &self.connection_pool
    }
}
