extern crate core;

mod sqlite_db;
pub mod types;

use anyhow::Error;
use casper_node::types::Block;
use casper_types::AsymmetricType;
use sqlite_db::Database;
use sse_client::EventSource;
use tracing::info;
use tracing_subscriber;
use types::structs::Config;
use types::enums::{Event, Network};

pub fn read_config(config_path: &str) -> Result<Config, Error> {
    let toml_content = std::fs::read_to_string(config_path)?;
    Ok(toml::from_str(&toml_content)?)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Install global collector for tracing
    tracing_subscriber::fmt::init();

    let config: Config = read_config("config.toml")?;
    info!("Configuration loaded");

    let node = match config.connection.network {
        Network::Mainnet => config.connection.node.mainnet,
        Network::Testnet => config.connection.node.testnet,
        Network::Local => config.connection.node.local,
    };

    let sse = EventSource::new(
        format!(
            "http://{ip}:{port}/events/{filter}",
            ip = node.ip_address,
            port = node.sse_port,
            filter = config.connection.sse_filter
        )
        .as_str(),
    )?;
    info!(
        message = "Connected to SSE",
        network = config.connection.network.as_str(),
        node_ip_address = node.ip_address.as_str()
    );

    // Create DB and create tables
    let storage: Database = Database::new(&config.storage.db_path)?;

    for (_index, event) in sse.receiver().iter().enumerate() {
        let json = serde_json::from_str(&event.data)?;

        match json {
            Event::ApiVersion(version) => {
                info!("API Version: {:?}", version);
            }
            Event::BlockAdded(block_added) => {
                let json_block = block_added.block;
                let block = Block::from(json_block);
                info!(
                    message = "Saving block:",
                    hash = hex::encode(block.hash().inner()).as_str(),
                    height = block.height()
                );
                storage.save_block(&block).await?;
            }
            Event::DeployAccepted(deploy_accepted) => {
                // deploy is represented only by hash in this event.
                // hence why the .deploy is actually just the hash.
                info!(
                    "Deploy Accepted: {}",
                    hex::encode(deploy_accepted.deploy.value())
                );
            }
            Event::DeployProcessed(deploy_processed) => {
                info!(
                    message = "Saving deploy:",
                    hash = hex::encode(deploy_processed.deploy_hash.value()).as_str()
                );
                storage.save_deploy(&deploy_processed).await?;
            }
            Event::Step(step) => {
                info!("\n\tStep reached for Era: {}", step.era_id);
                storage.save_step(&step).await?;
            }
            Event::Fault(fault) => {
                info!(
                    "\n\tFault reported!\n\tEra: {}\n\tPublic Key: {}\n\tTimestamp: {}",
                    fault.era_id,
                    fault.public_key.to_hex(),
                    fault.timestamp
                );
                storage.save_fault(&fault).await?;
            }
        }
    }

    Ok(())
}
