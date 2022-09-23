use casper_node::types::FinalitySignature as FinSig;

use anyhow::Error;
use async_trait::async_trait;
use sea_query::SqliteQueryBuilder;
use serde::Deserialize;
use sqlx::{sqlite::SqliteRow, Executor, Row, SqlitePool};

use super::{
    errors::{wrap_query_error, SqliteDbError},
    SqliteDatabase,
};
use crate::{
    sql::tables,
    types::{
        database::{AggregateDeployInfo, DatabaseReader, DatabaseRequestError},
        sse_events::*,
    },
};

#[async_trait]
impl DatabaseReader for SqliteDatabase {
    async fn get_latest_block(&self) -> Result<BlockAdded, DatabaseRequestError> {
        let db_connection = &self.connection_pool;

        let stmt = tables::block_added::create_get_latest_stmt().to_string(SqliteQueryBuilder);

        let row = fetch_optional_with_error_check(db_connection, stmt).await?;

        parse_block_from_row(row)
    }

    async fn get_block_by_height(&self, height: u64) -> Result<BlockAdded, DatabaseRequestError> {
        let db_connection = &self.connection_pool;

        let stmt =
            tables::block_added::create_get_by_height_stmt(height).to_string(SqliteQueryBuilder);

        let row = fetch_optional_with_error_check(db_connection, stmt).await?;

        parse_block_from_row(row)
    }

    async fn get_block_by_hash(&self, hash: &str) -> Result<BlockAdded, DatabaseRequestError> {
        let hash_regex = regex::Regex::new("^([A-Fa-f0-9]){64}$")
            .map_err(|err| DatabaseRequestError::Unhandled(Error::from(err)))?;
        if !hash_regex.is_match(hash) {
            return Err(DatabaseRequestError::InvalidParam(Error::msg(format!(
                "Expected hex-encoded block hash (64 chars), received: {} (length: {})",
                hash,
                hash.len()
            ))));
        }

        let db_connection = &self.connection_pool;

        let stmt = tables::block_added::create_get_by_hash_stmt(hash.to_string())
            .to_string(SqliteQueryBuilder);

        db_connection
            .fetch_optional(stmt.as_str())
            .await
            .map_err(|sql_err| DatabaseRequestError::Unhandled(Error::from(sql_err)))
            .and_then(|maybe_row| match maybe_row {
                None => Err(DatabaseRequestError::NotFound),
                Some(row) => parse_block_from_row(row),
            })
    }

    async fn get_latest_deploy_aggregate(
        &self,
    ) -> Result<AggregateDeployInfo, DatabaseRequestError> {
        let db_connection = &self.connection_pool;

        let stmt =
            tables::deploy_event::create_get_latest_deploy_hash().to_string(SqliteQueryBuilder);

        let latest_deploy_hash = db_connection
            .fetch_optional(stmt.as_str())
            .await
            .map_err(|sql_err| DatabaseRequestError::Unhandled(Error::from(sql_err)))
            .and_then(|maybe_row| match maybe_row {
                None => Err(DatabaseRequestError::NotFound),
                Some(row) => row
                    .try_get::<String, &str>("deploy_hash")
                    .map_err(|sqlx_err| wrap_query_error(sqlx_err.into())),
            })?;

        self.get_deploy_aggregate_by_hash(&latest_deploy_hash).await
    }

    async fn get_deploy_aggregate_by_hash(
        &self,
        hash: &str,
    ) -> Result<AggregateDeployInfo, DatabaseRequestError> {
        check_hash_is_correct_format(hash)?;
        // We may return here with NotFound because if there's no accepted record then theoretically there should be no other records for the given hash.
        let deploy_accepted = self.get_deploy_accepted_by_hash(hash).await?;

        // However we handle the Err case for DeployProcessed explicitly as we don't want to return NotFound when we've got a DeployAccepted to return
        match self.get_deploy_processed_by_hash(hash).await {
            Ok(deploy_processed) => Ok(AggregateDeployInfo {
                deploy_hash: hash.to_string(),
                deploy_accepted: Some(deploy_accepted),
                deploy_processed: Some(deploy_processed),
                deploy_expired: false,
            }),
            Err(err) => {
                // If the error is anything other than NotFound return the error.
                if !matches!(DatabaseRequestError::NotFound, _err) {
                    return Err(err);
                }
                match self.get_deploy_expired_by_hash(hash).await {
                    Ok(deploy_has_expired) => Ok(AggregateDeployInfo {
                        deploy_hash: hash.to_string(),
                        deploy_accepted: Some(deploy_accepted),
                        deploy_processed: None,
                        deploy_expired: deploy_has_expired,
                    }),
                    Err(err) => {
                        // If the error is anything other than NotFound return the error.
                        if !matches!(DatabaseRequestError::NotFound, _err) {
                            return Err(err);
                        }
                        Ok(AggregateDeployInfo {
                            deploy_hash: hash.to_string(),
                            deploy_accepted: Some(deploy_accepted),
                            deploy_processed: None,
                            deploy_expired: false,
                        })
                    }
                }
            }
        }
    }

    async fn get_deploy_accepted_by_hash(
        &self,
        hash: &str,
    ) -> Result<DeployAccepted, DatabaseRequestError> {
        check_hash_is_correct_format(hash)?;

        let db_connection = &self.connection_pool;

        let stmt = tables::deploy_accepted::create_get_by_hash_stmt(hash.to_string())
            .to_string(SqliteQueryBuilder);

        db_connection
            .fetch_optional(stmt.as_str())
            .await
            .map_err(|sql_err| DatabaseRequestError::Unhandled(Error::from(sql_err)))
            .and_then(|maybe_row| match maybe_row {
                None => Err(DatabaseRequestError::NotFound),
                Some(row) => {
                    let raw = row
                        .try_get::<String, &str>("raw")
                        .map_err(|sqlx_error| wrap_query_error(sqlx_error.into()))?;
                    deserialize_data::<DeployAccepted>(&raw).map_err(wrap_query_error)
                }
            })
    }

    async fn get_deploy_processed_by_hash(
        &self,
        hash: &str,
    ) -> Result<DeployProcessed, DatabaseRequestError> {
        check_hash_is_correct_format(hash)?;

        let db_connection = &self.connection_pool;

        let stmt = tables::deploy_processed::create_get_by_hash_stmt(hash.to_string())
            .to_string(SqliteQueryBuilder);

        db_connection
            .fetch_optional(stmt.as_str())
            .await
            .map_err(|sql_err| DatabaseRequestError::Unhandled(Error::from(sql_err)))
            .and_then(|maybe_row| match maybe_row {
                None => Err(DatabaseRequestError::NotFound),
                Some(row) => {
                    let raw = row
                        .try_get::<String, &str>("raw")
                        .map_err(|sqlx_error| wrap_query_error(sqlx_error.into()))?;
                    deserialize_data::<DeployProcessed>(&raw).map_err(wrap_query_error)
                }
            })
    }

    async fn get_deploy_expired_by_hash(&self, hash: &str) -> Result<bool, DatabaseRequestError> {
        check_hash_is_correct_format(hash)?;

        let db_connection = &self.connection_pool;

        let stmt = tables::deploy_expired::create_get_by_hash_stmt(hash.to_string())
            .to_string(SqliteQueryBuilder);

        db_connection
            .fetch_optional(stmt.as_str())
            .await
            .map_err(|sql_err| DatabaseRequestError::Unhandled(Error::from(sql_err)))
            .and_then(|maybe_row| match maybe_row {
                None => Err(DatabaseRequestError::NotFound),
                Some(_) => Ok(true),
            })
    }

    async fn get_step_by_era(&self, era: u64) -> Result<Step, DatabaseRequestError> {
        let db_connection = &self.connection_pool;

        let stmt = tables::step::create_get_by_era_stmt(era).to_string(SqliteQueryBuilder);

        db_connection
            .fetch_optional(stmt.as_str())
            .await
            .map_err(|sql_err| DatabaseRequestError::Unhandled(Error::from(sql_err)))
            .and_then(|maybe_row| match maybe_row {
                None => Err(DatabaseRequestError::NotFound),
                Some(row) => {
                    let raw = row
                        .try_get::<String, &str>("raw")
                        .map_err(|sqlx_error| wrap_query_error(sqlx_error.into()))?;
                    deserialize_data::<Step>(&raw).map_err(wrap_query_error)
                }
            })
    }

    async fn get_faults_by_public_key(
        &self,
        public_key: &str,
    ) -> Result<Vec<Fault>, DatabaseRequestError> {
        check_public_key_is_correct_format(public_key)?;

        let db_connection = &self.connection_pool;

        let stmt = tables::fault::create_get_faults_by_public_key_stmt(public_key.to_string())
            .to_string(SqliteQueryBuilder);

        db_connection
            .fetch_all(stmt.as_str())
            .await
            .map_err(|sql_err| DatabaseRequestError::Unhandled(Error::from(sql_err)))
            .and_then(parse_faults_from_rows)
    }

    async fn get_faults_by_era(&self, era: u64) -> Result<Vec<Fault>, DatabaseRequestError> {
        let db_connection = &self.connection_pool;

        let stmt = tables::fault::create_get_faults_by_era_stmt(era).to_string(SqliteQueryBuilder);

        db_connection
            .fetch_all(stmt.as_str())
            .await
            .map_err(|sql_err| DatabaseRequestError::Unhandled(Error::from(sql_err)))
            .and_then(parse_faults_from_rows)
    }

    async fn get_finality_signatures_by_block(
        &self,
        block_hash: &str,
    ) -> Result<Vec<FinSig>, DatabaseRequestError> {
        check_hash_is_correct_format(block_hash)?;

        let db_connection = &self.connection_pool;

        let stmt = tables::finality_signature::create_get_finality_signatures_by_block_stmt(
            block_hash.to_string(),
        )
        .to_string(SqliteQueryBuilder);

        db_connection
            .fetch_all(stmt.as_str())
            .await
            .map_err(|sql_err| DatabaseRequestError::Unhandled(Error::from(sql_err)))
            .and_then(parse_finality_signatures_from_rows)
    }
}

fn deserialize_data<'de, T: Deserialize<'de>>(data: &'de str) -> Result<T, SqliteDbError> {
    serde_json::from_str::<T>(data).map_err(SqliteDbError::SerdeJson)
}

fn parse_block_from_row(row: SqliteRow) -> Result<BlockAdded, DatabaseRequestError> {
    let raw_data = row
        .try_get::<String, &str>("raw")
        .map_err(|sqlx_err| wrap_query_error(sqlx_err.into()))?;
    deserialize_data::<BlockAdded>(&raw_data).map_err(wrap_query_error)
}

async fn fetch_optional_with_error_check(
    connection: &SqlitePool,
    stmt: String,
) -> Result<SqliteRow, DatabaseRequestError> {
    connection
        .fetch_optional(stmt.as_str())
        .await
        .map_err(|sql_err| DatabaseRequestError::Unhandled(Error::from(sql_err)))
        .and_then(|maybe_row| match maybe_row {
            None => Err(DatabaseRequestError::NotFound),
            Some(row) => Ok(row),
        })
}

fn parse_faults_from_rows(rows: Vec<SqliteRow>) -> Result<Vec<Fault>, DatabaseRequestError> {
    let mut faults = Vec::new();
    for row in rows {
        let raw = row
            .try_get::<String, &str>("raw")
            .map_err(|err| wrap_query_error(err.into()))?;

        let fault = deserialize_data::<Fault>(&raw).map_err(wrap_query_error)?;
        faults.push(fault);
    }

    if faults.is_empty() {
        return Err(DatabaseRequestError::NotFound);
    }
    Ok(faults)
}

fn parse_finality_signatures_from_rows(
    rows: Vec<SqliteRow>,
) -> Result<Vec<FinSig>, DatabaseRequestError> {
    let mut finality_signatures = Vec::new();
    for row in rows {
        let raw = row
            .try_get::<String, &str>("raw")
            .map_err(|err| wrap_query_error(err.into()))?;

        let finality_signature =
            deserialize_data::<FinalitySignature>(&raw).map_err(wrap_query_error)?;
        finality_signatures.push(finality_signature.inner());
    }

    if finality_signatures.is_empty() {
        return Err(DatabaseRequestError::NotFound);
    }
    Ok(finality_signatures)
}

fn check_hash_is_correct_format(hash: &str) -> Result<(), DatabaseRequestError> {
    let hash_regex = regex::Regex::new("^([0-9A-Fa-f]){64}$")
        .map_err(|err| DatabaseRequestError::Unhandled(Error::from(err)))?;
    if !hash_regex.is_match(hash) {
        return Err(DatabaseRequestError::InvalidParam(Error::msg(format!(
            "Expected hex-encoded hash (64 chars), received: {} (length: {})",
            hash,
            hash.len()
        ))));
    }
    Ok(())
}

fn check_public_key_is_correct_format(public_key_hex: &str) -> Result<(), DatabaseRequestError> {
    let public_key_regex = regex::Regex::new("^([0-9A-Fa-f]{2}){33,34}$")
        .map_err(|err| DatabaseRequestError::Unhandled(Error::from(err)))?;
    if !public_key_regex.is_match(public_key_hex) {
        Err(DatabaseRequestError::InvalidParam(Error::msg(format!(
            "Expected hex-encoded public key (66/68 chars), received: {} (length: {})",
            public_key_hex,
            public_key_hex.len()
        ))))
    } else {
        Ok(())
    }
}
