// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use super::{db_config::DbConfig, processor_config::ProcessorConfig};
use crate::{
    parquet_processors::{
        parquet_account_transactions_processor::ParquetAccountTransactionsProcessor,
        parquet_ans_processor::ParquetAnsProcessor,
        parquet_default_processor::ParquetDefaultProcessor,
        parquet_events_processor::ParquetEventsProcessor,
        parquet_fungible_asset_processor::ParquetFungibleAssetProcessor,
        parquet_objects_processor::ParquetObjectsProcessor,
        parquet_stake_processor::ParquetStakeProcessor,
        parquet_token_v2_processor::ParquetTokenV2Processor,
        parquet_transaction_metadata_processor::ParquetTransactionMetadataProcessor,
        parquet_user_transaction_processor::ParquetUserTransactionsProcessor,
    },
    processors::{
        account_restoration_processor::AccountRestorationProcessor,
        account_transactions_processor::AccountTransactionsProcessor, ans_processor::AnsProcessor,
        default_processor::DefaultProcessor, events_processor::EventsProcessor,
        fungible_asset_processor::FungibleAssetProcessor,
        monitoring_processor::MonitoringProcessor, objects_processor::ObjectsProcessor,
        stake_processor::StakeProcessor, token_v2_processor::TokenV2Processor,
        user_transaction_processor::UserTransactionProcessor,
    },
};
use anyhow::Result;
use aptos_indexer_processor_sdk::{
    aptos_indexer_transaction_stream::TransactionStreamConfig,
    traits::processor_trait::ProcessorTrait,
};
use aptos_indexer_processor_sdk_server_framework::RunnableConfig;
use serde::{Deserialize, Serialize};

pub const QUERY_DEFAULT_RETRIES: u32 = 5;
pub const QUERY_DEFAULT_RETRY_DELAY_MS: u64 = 500;

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub enum ProcessorMode {
    #[serde(rename = "default")]
    #[default]
    Default,
    #[serde(rename = "backfill")]
    Backfill,
    #[serde(rename = "testing")]
    Testing,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IndexerProcessorConfig {
    pub processor_config: ProcessorConfig,
    pub transaction_stream_config: TransactionStreamConfig,
    pub db_config: DbConfig,
    pub backfill_config: Option<BackfillConfig>,
    pub bootstrap_config: Option<BootStrapConfig>,
    pub testing_config: Option<TestingConfig>,
    #[serde(default)]
    pub mode: ProcessorMode,
}

impl IndexerProcessorConfig {
    fn validate(&self) -> Result<(), String> {
        match self.mode {
            ProcessorMode::Testing => {
                if self.testing_config.is_none() {
                    return Err("testing_config must be present when mode is 'testing'".to_string());
                }
            },
            ProcessorMode::Backfill => {
                if self.backfill_config.is_none() {
                    return Err(
                        "backfill_config must be present when mode is 'backfill'".to_string()
                    );
                }
            },
            ProcessorMode::Default => {},
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for IndexerProcessorConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Inner {
            processor_config: ProcessorConfig,
            transaction_stream_config: TransactionStreamConfig,
            db_config: DbConfig,
            backfill_config: Option<BackfillConfig>,
            bootstrap_config: Option<BootStrapConfig>,
            testing_config: Option<TestingConfig>,
            #[serde(default)]
            mode: ProcessorMode,
        }

        let inner = Inner::deserialize(deserializer)?;
        let config = IndexerProcessorConfig {
            processor_config: inner.processor_config,
            transaction_stream_config: inner.transaction_stream_config,
            db_config: inner.db_config,
            backfill_config: inner.backfill_config,
            bootstrap_config: inner.bootstrap_config,
            testing_config: inner.testing_config,
            mode: inner.mode,
        };

        config.validate().map_err(serde::de::Error::custom)?;
        Ok(config)
    }
}

#[async_trait::async_trait]
impl RunnableConfig for IndexerProcessorConfig {
    async fn run(&self) -> Result<()> {
        match self.processor_config {
            ProcessorConfig::AccountRestorationProcessor(_) => {
                let acc_rest_processor = AccountRestorationProcessor::new(self.clone()).await?;
                acc_rest_processor.run_processor().await
            },
            ProcessorConfig::AccountTransactionsProcessor(_) => {
                let acc_txns_processor = AccountTransactionsProcessor::new(self.clone()).await?;
                acc_txns_processor.run_processor().await
            },
            ProcessorConfig::AnsProcessor(_) => {
                let ans_processor = AnsProcessor::new(self.clone()).await?;
                ans_processor.run_processor().await
            },
            ProcessorConfig::DefaultProcessor(_) => {
                let default_processor = DefaultProcessor::new(self.clone()).await?;
                default_processor.run_processor().await
            },
            ProcessorConfig::EventsProcessor(_) => {
                let events_processor = EventsProcessor::new(self.clone()).await?;
                events_processor.run_processor().await
            },
            ProcessorConfig::FungibleAssetProcessor(_) => {
                let fungible_asset_processor = FungibleAssetProcessor::new(self.clone()).await?;
                fungible_asset_processor.run_processor().await
            },
            ProcessorConfig::UserTransactionProcessor(_) => {
                let user_txns_processor = UserTransactionProcessor::new(self.clone()).await?;
                user_txns_processor.run_processor().await
            },
            ProcessorConfig::StakeProcessor(_) => {
                let stake_processor = StakeProcessor::new(self.clone()).await?;
                stake_processor.run_processor().await
            },
            ProcessorConfig::MonitoringProcessor(_) => {
                let monitoring_processor = MonitoringProcessor::new(self.clone()).await?;
                monitoring_processor.run_processor().await
            },
            ProcessorConfig::TokenV2Processor(_) => {
                let token_v2_processor = TokenV2Processor::new(self.clone()).await?;
                token_v2_processor.run_processor().await
            },
            ProcessorConfig::ObjectsProcessor(_) => {
                let objects_processor = ObjectsProcessor::new(self.clone()).await?;
                objects_processor.run_processor().await
            },
            ProcessorConfig::ParquetDefaultProcessor(_) => {
                let parquet_default_processor = ParquetDefaultProcessor::new(self.clone()).await?;
                parquet_default_processor.run_processor().await
            },
            ProcessorConfig::ParquetEventsProcessor(_) => {
                let parquet_events_processor = ParquetEventsProcessor::new(self.clone()).await?;
                parquet_events_processor.run_processor().await
            },
            ProcessorConfig::ParquetUserTransactionsProcessor(_) => {
                let parquet_user_transactions_processor =
                    ParquetUserTransactionsProcessor::new(self.clone()).await?;
                parquet_user_transactions_processor.run_processor().await
            },
            ProcessorConfig::ParquetFungibleAssetProcessor(_) => {
                let parquet_fungible_asset_processor =
                    ParquetFungibleAssetProcessor::new(self.clone()).await?;
                parquet_fungible_asset_processor.run_processor().await
            },
            ProcessorConfig::ParquetTransactionMetadataProcessor(_) => {
                let parquet_transaction_metadata_processor =
                    ParquetTransactionMetadataProcessor::new(self.clone()).await?;
                parquet_transaction_metadata_processor.run_processor().await
            },
            ProcessorConfig::ParquetAccountTransactionsProcessor(_) => {
                let parquet_account_transactions_processor =
                    ParquetAccountTransactionsProcessor::new(self.clone()).await?;
                parquet_account_transactions_processor.run_processor().await
            },
            ProcessorConfig::ParquetTokenV2Processor(_) => {
                let parquet_token_v2_processor = ParquetTokenV2Processor::new(self.clone()).await?;
                parquet_token_v2_processor.run_processor().await
            },
            ProcessorConfig::ParquetAnsProcessor(_) => {
                let parquet_ans_processor = ParquetAnsProcessor::new(self.clone()).await?;
                parquet_ans_processor.run_processor().await
            },
            ProcessorConfig::ParquetStakeProcessor(_) => {
                let parquet_stake_processor = ParquetStakeProcessor::new(self.clone()).await?;
                parquet_stake_processor.run_processor().await
            },
            ProcessorConfig::ParquetObjectsProcessor(_) => {
                let parquet_objects_processor = ParquetObjectsProcessor::new(self.clone()).await?;
                parquet_objects_processor.run_processor().await
            },
        }
    }

    fn get_server_name(&self) -> String {
        // Get the part before the first _ and trim to 12 characters.
        let before_underscore = self
            .processor_config
            .name()
            .split('_')
            .next()
            .unwrap_or("unknown");
        before_underscore[..before_underscore.len().min(12)].to_string()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackfillConfig {
    pub backfill_id: String,
    pub initial_starting_version: u64,
    pub ending_version: u64,
    pub overwrite_checkpoint: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Initial starting version for non-backfill processors. Processors will pick up where it left off
/// if restarted. Read more in `starting_version.rs`
pub struct BootStrapConfig {
    pub initial_starting_version: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Use this config for testing. Processors will not use checkpoint and will
/// always start from `override_starting_version`.
pub struct TestingConfig {
    pub override_starting_version: u64,
    pub ending_version: u64,
}
