extern crate core;

mod event_stream_server;
mod rest_server;
mod sql;
mod sqlite_database;
#[cfg(test)]
mod testing;
mod types;
mod utils;

use std::path::{Path, PathBuf};

use anyhow::{Context, Error};
use hex_fmt::HexFmt;
use tracing::{debug, info, warn};

use casper_event_listener::EventListener;
use casper_event_types::SseData;

use crate::{
    event_stream_server::{Config as SseConfig, EventStreamServer},
    rest_server::run_server as start_rest_server,
    sqlite_database::SqliteDatabase,
    types::{config::Config, database::DatabaseWriter, sse_events::*},
};

const CONFIG_PATH: &str = "config.toml";

pub fn read_config(config_path: &str) -> Result<Config, Error> {
    let toml_content =
        std::fs::read_to_string(config_path).context("Error reading config file contents")?;
    toml::from_str(&toml_content).context("Error parsing config into TOML format")
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Install global collector for tracing
    tracing_subscriber::fmt::init();

    let config: Config = read_config(CONFIG_PATH).context("Error constructing config")?;
    info!("Configuration loaded");

    run(config).await
}

async fn run(config: Config) -> Result<(), Error> {
    let bind_address = format!(
        "{}:{}",
        config.node_connection.ip_address, config.node_connection.sse_port
    );

    let event_listener = EventListener::new(
        bind_address,
        config.node_connection.max_retries,
        config.node_connection.delay_between_retries_in_seconds,
    )
    .await?;

    let path_to_database_dir = Path::new(&config.storage.storage_path);

    // Creates and initialises Sqlite database
    let sqlite_database =
        SqliteDatabase::new(path_to_database_dir, config.storage.sqlite_config.clone())
            .await
            .context("Error instantiating database")?;

    // Prepare the REST server task - this will be executed later
    let rest_server_handle = tokio::spawn(start_rest_server(
        config.rest_server.ip_address,
        config.rest_server.port,
        sqlite_database.file_path.clone(),
        config.storage.sqlite_config.max_read_connections,
    ));

    let event_stream_server_address = format!(
        "{}:{}",
        config.event_stream_server.ip_address, config.event_stream_server.port
    );

    // Create new instance for the Sidecar's Event Stream Server
    let event_stream_server = EventStreamServer::new(
        SseConfig::new(
            Some(event_stream_server_address),
            Some(config.event_stream_server.event_stream_buffer_length),
            Some(config.event_stream_server.max_concurrent_subscribers),
        ),
        PathBuf::from(config.storage.storage_path),
        event_listener.api_version,
    )
    .context("Error starting EventStreamServer")?;

    // Adds space under setup logs before stream starts for readability
    println!("\n\n");

    let sse_processing_task = tokio::spawn(sse_processor(
        event_listener,
        event_stream_server,
        sqlite_database,
    ));

    tokio::select! {
        _ = sse_processing_task => {
            info!("Stopped processing SSEs")
        }

        _ = rest_server_handle => {
            info!("REST server stopped")
        }
    }

    Ok(())
}

async fn sse_processor(
    sse_event_listener: EventListener,
    mut event_stream_server: EventStreamServer,
    sqlite_database: SqliteDatabase,
) {
    let mut sse_data_stream = sse_event_listener.consume_combine_streams().await;

    while let Some(sse_event) = sse_data_stream.recv().await {
        match sse_event.data {
            SseData::ApiVersion(version) => info!(%version, "API Version"),
            SseData::BlockAdded { block, block_hash } => {
                info!("Block Added: {:18}", HexFmt(block_hash.inner()));
                let res = sqlite_database
                    .save_block_added(
                        BlockAdded::new(block_hash, block.clone()),
                        sse_event.id.unwrap(),
                        sse_event.source,
                    )
                    .await;

                match res {
                    Ok(_) => {
                        event_stream_server.broadcast(SseData::BlockAdded { block, block_hash })
                    }
                    Err(err) => warn!(?err, "Unexpected error saving BlockAdded"),
                }
            }
            SseData::DeployAccepted { deploy } => {
                info!("Deploy Accepted: {:18}", HexFmt(deploy.id().inner()));
                let deploy_accepted = DeployAccepted::new(deploy.clone());
                let res = sqlite_database
                    .save_deploy_accepted(deploy_accepted, sse_event.id.unwrap(), sse_event.source)
                    .await;

                match res {
                    Ok(_) => event_stream_server.broadcast(SseData::DeployAccepted { deploy }),
                    Err(err) => warn!(?err, "Unexpected error saving DeployAccepted"),
                }
            }
            SseData::DeployExpired { deploy_hash } => {
                info!("Deploy Expired: {:18}", HexFmt(deploy_hash.inner()));
                let res = sqlite_database
                    .save_deploy_expired(
                        DeployExpired::new(deploy_hash),
                        sse_event.id.unwrap(),
                        sse_event.source,
                    )
                    .await;

                match res {
                    Ok(_) => event_stream_server.broadcast(SseData::DeployExpired { deploy_hash }),
                    Err(err) => warn!(?err, "Unexpected error saving DeployExpired"),
                }
            }
            SseData::DeployProcessed {
                deploy_hash,
                account,
                timestamp,
                ttl,
                dependencies,
                block_hash,
                execution_result,
            } => {
                info!("Deploy Processed: {:18}", HexFmt(deploy_hash.inner()));
                let deploy_processed = DeployProcessed::new(
                    deploy_hash.clone(),
                    account.clone(),
                    timestamp,
                    ttl,
                    dependencies.clone(),
                    block_hash.clone(),
                    execution_result.clone(),
                );
                let res = sqlite_database
                    .save_deploy_processed(
                        deploy_processed.clone(),
                        sse_event.id.unwrap(),
                        sse_event.source,
                    )
                    .await;

                match res {
                    Ok(_) => event_stream_server.broadcast(SseData::DeployProcessed {
                        deploy_hash,
                        account,
                        timestamp,
                        ttl,
                        dependencies,
                        block_hash,
                        execution_result,
                    }),
                    Err(err) => warn!(?err, "Unexpected error saving DeployProcessed"),
                }
            }
            SseData::Fault {
                era_id,
                timestamp,
                public_key,
            } => {
                let fault = Fault::new(era_id, public_key.clone(), timestamp);
                warn!(%fault, "Fault reported");
                let res = sqlite_database
                    .save_fault(fault.clone(), sse_event.id.unwrap(), sse_event.source)
                    .await;

                match res {
                    Ok(_) => event_stream_server.broadcast(SseData::Fault {
                        era_id,
                        timestamp,
                        public_key,
                    }),
                    Err(err) => warn!(?err, "Unexpected error saving Fault"),
                }
            }
            SseData::FinalitySignature(fs) => {
                debug!("Finality Signature: {} for {}", fs.signature, fs.block_hash);
                let finality_signature = FinalitySignature::new(fs.clone());
                let res = sqlite_database
                    .save_finality_signature(
                        finality_signature.clone(),
                        sse_event.id.unwrap(),
                        sse_event.source,
                    )
                    .await;

                match res {
                    Ok(_) => event_stream_server.broadcast(SseData::FinalitySignature(fs)),
                    Err(err) => warn!(?err, "Unexpected error saving FinalitySignature"),
                }
            }
            SseData::Step {
                era_id,
                execution_effect,
            } => {
                let step = Step::new(era_id, execution_effect.clone());
                info!("Step at era: {}", era_id.value());
                let res = sqlite_database
                    .save_step(step, sse_event.id.unwrap(), sse_event.source)
                    .await;

                match res {
                    Ok(_) => event_stream_server.broadcast(SseData::Step {
                        era_id,
                        execution_effect,
                    }),
                    Err(err) => warn!(?err, "Unexpected error saving Step"),
                }
            }
            SseData::Shutdown => {
                warn!("Node ({}) is unavailable", sse_event.source);
                break;
            }
        }
    }
}

/// A convenience wrapper around [Config] with a [Drop] impl that removes the `test_storage` dir created in `target` during testing.
/// This means there is no need to explicitly remove the directory at the end of the tests which is liable to be skipped if the test fails earlier.
#[cfg(test)]
struct ConfigWithCleanup {
    config: Config,
}

#[cfg(test)]
impl ConfigWithCleanup {
    fn new(path: &str) -> Self {
        let config = read_config(path).expect("Error parsing config file");
        Self { config }
    }
}

#[cfg(test)]
impl Drop for ConfigWithCleanup {
    fn drop(&mut self) {
        let path_to_test_storage = Path::new(&self.config.storage.storage_path);
        if path_to_test_storage.exists() {
            let res = std::fs::remove_dir_all(path_to_test_storage);
            if let Err(error) = res {
                println!("Error removing test_storage dir: {}", error);
            }
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use crate::read_config;

    #[test]
    fn should_parse_config_toml_files() {
        read_config("../config.toml").expect("Error parsing config.toml");
        read_config("config_test.toml").expect("Error parsing config_test.toml");
        read_config("config_perf_test.toml").expect("Error parsing config_perf_test.toml");
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::testing::mock_node::start_test_node_with_shutdown;
    use eventsource_stream::Eventsource;
    use serial_test::serial;
    use std::time::Duration;
    use tokio_stream::StreamExt;

    const TEST_CONFIG_PATH: &str = "config_test.toml";

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial]
    #[ignore]
    async fn should_connect_and_shutdown_cleanly() {
        let node_shutdown_tx = start_test_node_with_shutdown(4444, None).await;

        let test_config = ConfigWithCleanup::new(TEST_CONFIG_PATH);

        run(test_config.config.clone())
            .await
            .expect("Error running sidecar");

        node_shutdown_tx.send(()).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial]
    #[ignore]
    async fn should_allow_client_connection_to_sse() {
        let node_shutdown_tx = start_test_node_with_shutdown(4444, Some(30)).await;

        let test_config = ConfigWithCleanup::new(TEST_CONFIG_PATH);

        tokio::spawn(run(test_config.config.clone()));

        // Allow sidecar to spin up
        tokio::time::sleep(Duration::from_secs(3)).await;

        let mut main_event_stream = reqwest::Client::new()
            .get("http://127.0.0.1:19999/events/main")
            .send()
            .await
            .expect("Error in main event stream")
            .bytes_stream()
            .eventsource();

        while let Some(event) = main_event_stream.next().await {
            event.expect("Error from event stream - event should have been OK");
        }

        node_shutdown_tx.send(()).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial]
    #[ignore]
    async fn should_respond_to_rest_query() {
        let node_shutdown_tx = start_test_node_with_shutdown(4444, Some(30)).await;

        let test_config = ConfigWithCleanup::new(TEST_CONFIG_PATH);

        tokio::spawn(run(test_config.config.clone()));

        // Allow sidecar to spin up
        tokio::time::sleep(Duration::from_secs(3)).await;

        let response = reqwest::Client::new()
            .get("http://127.0.0.1:17777/block")
            .send()
            .await
            .expect("Error requesting the /block endpoint");

        assert!(response.status().is_success());

        node_shutdown_tx.send(()).unwrap();
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    use bytes::Bytes;
    use eventsource_stream::{EventStream, Eventsource};
    use hex::encode;
    use serial_test::serial;
    use std::println;
    use std::time::Duration;
    use tokio::time::Instant;
    use tokio_stream::{Stream, StreamExt};

    #[derive(Clone)]
    struct EventWithHash {
        hash: String,
        received_at: Instant,
    }

    impl PartialEq for EventWithHash {
        fn eq(&self, other: &Self) -> bool {
            self.hash == other.hash
        }
    }

    const PERF_TEST_CONFIG_PATH: &str = "config_perf_test.toml";
    const EVENT_COUNT: u8 = 30;
    const ACCEPTABLE_LAG_IN_MILLIS: u128 = 1000;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial]
    #[ignore]
    // This test needs NCTL running in the background
    async fn check_delay_in_receiving_blocks() {
        let perf_test_config = ConfigWithCleanup::new(PERF_TEST_CONFIG_PATH);

        tokio::spawn(run(perf_test_config.config.clone()));

        // Allow sidecar to spin up
        tokio::time::sleep(Duration::from_secs(3)).await;

        let node_event_stream = reqwest::Client::new()
            .get("http://127.0.0.1:18101/events/main")
            .send()
            .await
            .expect("Error connecting to node")
            .bytes_stream()
            .eventsource();

        let sidecar_event_stream = reqwest::Client::new()
            .get("http://127.0.0.1:19999/events/main")
            .send()
            .await
            .expect("Error connecting to sidecar")
            .bytes_stream()
            .eventsource();

        let node_task_handle =
            tokio::spawn(push_timestamped_block_events_to_vecs(node_event_stream));

        let sidecar_task_handle =
            tokio::spawn(push_timestamped_block_events_to_vecs(sidecar_event_stream));

        let (node_task_result, sidecar_task_result) =
            tokio::join!(node_task_handle, sidecar_task_handle);

        let (block_events_from_node, node_overall_duration) =
            node_task_result.expect("Error recording events from node");
        let (block_events_from_sidecar, sidecar_overall_duration) =
            sidecar_task_result.expect("Error recording events from sidecar");

        let block_time_diffs =
            extract_time_diffs(block_events_from_node, block_events_from_sidecar);

        let block_time_diff_millis = block_time_diffs
            .iter()
            .map(|time_diff| {
                println!(
                    "Block Time Diff: {} micros / {} ms",
                    time_diff.as_micros(),
                    time_diff.as_millis()
                );
                time_diff.as_millis()
            })
            .collect::<Vec<u128>>();

        let average_delay: u128 = block_time_diff_millis
            .iter()
            .sum::<u128>()
            .checked_div(block_time_diff_millis.len() as u128)
            .expect("Error calculating the average delay for blocks");

        println!(
            "\n\tBLOCKS RESULT:\n\
        \tAverage delay taken over {} matching block diffs = {} ms\n\
        \tOverall difference in time to receive {} events = {}ms\t (sidecar: {}s, node: {}s)\n",
            block_time_diff_millis.len(),
            average_delay,
            EVENT_COUNT,
            sidecar_overall_duration
                .as_millis()
                .checked_sub(node_overall_duration.as_millis())
                .expect("Error taking the difference in the overall durations"),
            sidecar_overall_duration.as_secs(),
            node_overall_duration.as_secs()
        );

        assert!(average_delay < ACCEPTABLE_LAG_IN_MILLIS);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[serial]
    #[ignore]
    // This test needs NCTL running in the background with deploys being sent
    async fn check_delay_in_receiving_deploys() {
        let perf_test_config = ConfigWithCleanup::new(PERF_TEST_CONFIG_PATH);

        tokio::spawn(run(perf_test_config.config.clone()));

        // Allow sidecar to spin up
        tokio::time::sleep(Duration::from_secs(3)).await;

        let node_event_stream = reqwest::Client::new()
            .get("http://127.0.0.1:18101/events/deploys")
            .send()
            .await
            .expect("Error connecting to node")
            .bytes_stream()
            .eventsource();

        let sidecar_event_stream = reqwest::Client::new()
            .get("http://127.0.0.1:19999/events/deploys")
            .send()
            .await
            .expect("Error connecting to sidecar")
            .bytes_stream()
            .eventsource();

        let node_task_handle =
            tokio::spawn(push_timestamped_deploy_events_to_vecs(node_event_stream));

        let sidecar_task_handle =
            tokio::spawn(push_timestamped_deploy_events_to_vecs(sidecar_event_stream));

        let (node_task_result, sidecar_task_result) =
            tokio::join!(node_task_handle, sidecar_task_handle);

        let (deploy_events_from_node, node_overall_duration) =
            node_task_result.expect("Error recording events from node");
        let (deploy_events_from_sidecar, sidecar_overall_duration) =
            sidecar_task_result.expect("Error recording events from sidecar");

        assert_eq!(deploy_events_from_node.len(), deploy_events_from_node.len());

        let deploy_time_diffs =
            extract_time_diffs(deploy_events_from_node, deploy_events_from_sidecar);

        let deploy_time_diff_millis = deploy_time_diffs
            .iter()
            .map(|time_diff| {
                println!(
                    "Deploy Time Diff: {} micros / {} ms",
                    time_diff.as_micros(),
                    time_diff.as_millis()
                );
                time_diff.as_millis()
            })
            .collect::<Vec<u128>>();

        let average_delay: u128 = deploy_time_diff_millis
            .iter()
            .sum::<u128>()
            .checked_div(deploy_time_diff_millis.len() as u128)
            .expect("Error calculating the average delay for blocks");

        println!(
            "\n\tDEPLOYS RESULT:\n\
        \tAverage delay taken over {} matching deploy diffs = {} ms\n\
        \tOverall difference in time to receive {} events = {}ms\t (sidecar: {}s, node: {}s)\n",
            deploy_time_diff_millis.len(),
            average_delay,
            EVENT_COUNT,
            sidecar_overall_duration
                .as_millis()
                .checked_sub(node_overall_duration.as_millis())
                .expect("Error taking the difference in the overall duration"),
            sidecar_overall_duration.as_secs(),
            node_overall_duration.as_secs()
        );

        assert!(average_delay < ACCEPTABLE_LAG_IN_MILLIS);
    }

    async fn push_timestamped_block_events_to_vecs(
        mut event_stream: EventStream<impl Stream<Item = Result<Bytes, reqwest::Error>> + Unpin>,
    ) -> (Vec<EventWithHash>, Duration) {
        let mut events_vec = Vec::new();

        let mut events_read = 0u8;

        let before = Instant::now();

        while let Some(event) = event_stream.next().await {
            let received_timestamp = Instant::now();
            let data = serde_json::from_str::<SseData>(
                &event.expect("Received error from event stream").data,
            )
            .expect("Error deserialising the event into SseData");
            if let SseData::BlockAdded { block_hash, .. } = data {
                events_read += 1;
                let hash = encode(block_hash.inner());
                events_vec.push(EventWithHash {
                    hash,
                    received_at: received_timestamp,
                });
            }
            if events_read >= EVENT_COUNT {
                break;
            }
        }

        let after = Instant::now();

        (events_vec, after.duration_since(before))
    }

    async fn push_timestamped_deploy_events_to_vecs(
        mut event_stream: EventStream<impl Stream<Item = Result<Bytes, reqwest::Error>> + Unpin>,
    ) -> (Vec<EventWithHash>, Duration) {
        let mut events_vec = Vec::new();

        let mut events_read = 0u8;

        let before = Instant::now();

        while let Some(event) = event_stream.next().await {
            let received_timestamp = Instant::now();
            let data = serde_json::from_str::<SseData>(
                &event.expect("Received error from event stream").data,
            )
            .expect("Error deserialising the event into SseData");
            if let SseData::DeployAccepted { deploy } = data {
                events_read += 1;
                let hash = encode(*deploy.id());
                events_vec.push(EventWithHash {
                    hash,
                    received_at: received_timestamp,
                })
            }
            if events_read >= EVENT_COUNT {
                break;
            }
        }

        let after = Instant::now();

        (events_vec, after.duration_since(before))
    }

    fn extract_time_diffs(
        events_from_node: Vec<EventWithHash>,
        events_from_sidecar: Vec<EventWithHash>,
    ) -> Vec<Duration> {
        events_from_node
            .iter()
            .map(|event_from_node| {
                let cloned_events_from_sidecar = events_from_sidecar.clone();
                cloned_events_from_sidecar
                    .iter()
                    .map(|event_from_sidecar| {
                        if event_from_sidecar.eq(event_from_node) {
                            let time_difference =
                                event_from_sidecar.received_at - event_from_node.received_at;
                            return Some(time_difference);
                        }
                        None
                    })
                    .reduce(
                        |previous, current| {
                            if current.is_some() {
                                current
                            } else {
                                previous
                            }
                        },
                    )
                    .map(|reduced| reduced.expect("Reducer failed to get the Duration"))
            })
            .map(|opt_time_difference| {
                opt_time_difference.expect("Duration should have been populated")
            })
            .collect::<Vec<Duration>>()
    }
}
