use crate::types::enums::Network;
use casper_hashing::Digest;
use casper_node::types::{BlockHash, JsonBlock, TimeDiff, Timestamp};
use casper_types::account::AccountHash;
use casper_types::{AccessRights, AsymmetricType, DeployHash, EraId, ExecutionEffect, ExecutionResult, PublicKey, Transfer, TransferAddr, Transform, TransformEntry, U512, URef};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct Config {
    pub connection: ConnectionConfig,
    pub storage: StorageConfig,
    pub rest_server: ServerConfig,
    pub ws_server: ServerConfig,
}

#[derive(Deserialize)]
pub struct ServerConfig {
    pub run: bool,
    pub port: u16,
}

#[derive(Deserialize)]
pub struct StorageConfig {
    pub db_path: String,
    pub kv_path: String,
}

#[derive(Deserialize)]
pub struct ConnectionConfig {
    pub network: Network,
    pub node: Node,
    pub sse_filter: String,
}

#[derive(Deserialize)]
pub struct Node {
    pub testnet: NodeConfig,
    pub mainnet: NodeConfig,
    pub local: NodeConfig,
}

#[derive(Deserialize)]
pub struct NodeConfig {
    pub ip_address: String,
    pub sse_port: u16,
    pub rpc_port: u16,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BlockAdded {
    pub block_hash: String,
    pub block: JsonBlock,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DeployAccepted {
    pub deploy: DeployHash,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DeployProcessed {
    pub deploy_hash: Box<DeployHash>,
    account: Box<PublicKey>,
    pub timestamp: Timestamp,
    ttl: TimeDiff,
    dependencies: Vec<DeployHash>,
    pub block_hash: Box<BlockHash>,
    pub execution_result: Box<ExecutionResult>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Step {
    pub era_id: EraId,
    pub execution_effect: ExecutionEffect,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Fault {
    pub era_id: EraId,
    pub public_key: PublicKey,
    pub timestamp: Timestamp,
}

pub trait New {
    fn new_with_transfers() -> Self;
    fn new_without_transfers() -> Self;
    fn new_populated_transfers() -> Self;
}

impl New for DeployProcessed {
    fn new_with_transfers() -> Self {
        let transforms = vec![
            TransformEntry {
                key: "uref-2c4a11c062a8a337bfc97e27fd66291caeb2c65865dcb5d3ef3759c4c97efecb-007"
                    .to_string(),
                transform: Transform::AddUInt64(8u64),
            },
            TransformEntry {
                key: "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1"
                    .to_string(),
                transform: Transform::Identity,
            },
            TransformEntry {
                key: "transfer-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1"
                    .to_string(),
                transform: Transform::WriteTransfer(Transfer::default()),
            },
            TransformEntry {
                key: "transfer-2c4a11c062a8a337bfc97e27fd66291caeb2c65865dcb5d3ef3759c4c97efecb"
                    .to_string(),
                transform: Transform::WriteTransfer(Transfer::default()),
            },
        ];

        let effect = ExecutionEffect {
            operations: Default::default(),
            transforms,
        };

        DeployProcessed {
            deploy_hash: Box::new(DeployHash::new(<[u8; 32]>::from(Digest::hash(vec![
                1u8;
                32
            ])))),
            account: Box::new(
                PublicKey::from_hex(
                    "01af29904f39610dc410844122e35a22d134fb199eaa82ca0f29324605ac2f47f2",
                )
                .expect("should create public key from hex"),
            ),
            timestamp: Timestamp::now(),
            ttl: TimeDiff::default(),
            dependencies: Default::default(),
            block_hash: Box::new(BlockHash::new(Digest::hash(vec![1u8; 32]))),
            execution_result: Box::new(ExecutionResult::Success {
                effect,
                transfers: vec![TransferAddr::default(); 2],
                cost: Default::default(),
            }),
        }
    }

    fn new_without_transfers() -> Self {
        let transforms = vec![
            TransformEntry {
                key: "uref-2c4a11c062a8a337bfc97e27fd66291caeb2c65865dcb5d3ef3759c4c97efecb-007"
                    .to_string(),
                transform: Transform::AddUInt64(8u64),
            },
            TransformEntry {
                key: "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1"
                    .to_string(),
                transform: Transform::Identity,
            },
        ];

        let effect = ExecutionEffect {
            operations: Default::default(),
            transforms,
        };

        DeployProcessed {
            deploy_hash: Box::new(DeployHash::new(<[u8; 32]>::from(Digest::hash(vec![
                1u8;
                32
            ])))),
            account: Box::new(
                PublicKey::from_hex(
                    "01af29904f39610dc410844122e35a22d134fb199eaa82ca0f29324605ac2f47f2",
                )
                .expect("should create public key from hex"),
            ),
            timestamp: Timestamp::now(),
            ttl: TimeDiff::default(),
            dependencies: Default::default(),
            block_hash: Box::new(BlockHash::new(Digest::hash(vec![1u8; 32]))),
            execution_result: Box::new(ExecutionResult::Success {
                effect,
                transfers: Default::default(),
                cost: Default::default(),
            }),
        }
    }

    fn new_populated_transfers() -> Self {
        let public_key_a = PublicKey::from_hex(
            "01af29904f39610dc410844122e35a22d134fb199eaa82ca0f29324605ac2f47f2",
        )
        .expect("should create public key from hex");
        let public_key_b = PublicKey::from_hex(
            "01fe9f09d67ac6697d6617847697e45f549259152716830dad2cd7a1b7c3b8dd6f",
        )
        .expect("should create public key from hex");
        let public_key_c = PublicKey::from_hex(
            "01a124225ccefd31aed6669352635304acff29d426e5ce0ef0bbee2f147fb9d3d3",
        )
        .expect("should create public key from hex");
        let public_key_d = PublicKey::from_hex(
            "01926bdc35220afca31affa30fd5b6c176a2afce4d805caaf5f7aa8e7fd0c9d47a",
        )
        .expect("should create public key from hex");

        let purse_uref_a = URef::new([1u8; 32], AccessRights::default());
        let purse_uref_b = URef::new([2u8; 32], AccessRights::default());
        let purse_uref_c = URef::new([3u8; 32], AccessRights::default());
        let purse_uref_d = URef::new([4u8; 32], AccessRights::default());

        let transforms = vec![
            TransformEntry {
                key: "uref-2c4a11c062a8a337bfc97e27fd66291caeb2c65865dcb5d3ef3759c4c97efecb-007"
                    .to_string(),
                transform: Transform::AddUInt64(8u64),
            },
            TransformEntry {
                key: "deploy-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1"
                    .to_string(),
                transform: Transform::Identity,
            },
            TransformEntry {
                key: "transfer-af684263911154d26fa05be9963171802801a0b6aff8f199b7391eacb8edc9e1"
                    .to_string(),
                transform: Transform::WriteTransfer(Transfer {
                    deploy_hash: Default::default(),
                    from: AccountHash::from(&public_key_a),
                    to: Some(AccountHash::from(&public_key_b)),
                    source: purse_uref_a,
                    target: purse_uref_b,
                    amount: U512::from(13042001),
                    gas: Default::default(),
                    id: None,
                }),
            },
            TransformEntry {
                key: "transfer-2c4a11c062a8a337bfc97e27fd66291caeb2c65865dcb5d3ef3759c4c97efecb"
                    .to_string(),
                transform: Transform::WriteTransfer(Transfer {
                    deploy_hash: Default::default(),
                    from: AccountHash::from(&public_key_c),
                    to: Some(AccountHash::from(&public_key_d)),
                    source: purse_uref_c,
                    target: purse_uref_d,
                    amount: U512::from(23041999),
                    gas: Default::default(),
                    id: None,
                }),
            },
        ];

        let effect = ExecutionEffect {
            operations: Default::default(),
            transforms,
        };

        DeployProcessed {
            deploy_hash: Box::new(DeployHash::new(<[u8; 32]>::from(Digest::hash(vec![
                1u8;
                32
            ])))),
            account: Box::new(public_key_a),
            timestamp: Timestamp::now(),
            ttl: TimeDiff::default(),
            dependencies: Default::default(),
            block_hash: Box::new(BlockHash::new(Digest::hash(vec![1u8; 32]))),
            execution_result: Box::new(ExecutionResult::Success {
                effect,
                transfers: vec![TransferAddr::default(); 2],
                cost: Default::default(),
            }),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StateRootHashRpcResult {
    api_version: String,
    pub(crate) state_root_hash: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BalanceRpcResult {
    api_version: String,
    pub(crate) balance_value: String,
    merkle_proof: String,
}
