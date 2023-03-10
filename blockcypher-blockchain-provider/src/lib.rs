use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bitcoin::consensus::Decodable;
use bitcoin::hashes::hex::FromHex;
use bitcoin::util::uint::Uint256;
use bitcoin::{
    Block, BlockHash, BlockHeader, Network, OutPoint, Script, Transaction, TxMerkleNode, TxOut,
    Txid,
};
use bitcoin_test_utils::tx_to_string;
use chrono::DateTime;
use dlc_manager::{error::Error, Blockchain, Utxo};
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};
use lightning_block_sync::{BlockData, BlockHeaderData, BlockSource, BlockSourceError};
use reqwest::blocking::Response;
use serde::Deserialize;
use serde::Serialize;

const MIN_FEERATE: u32 = 253;

#[derive(Clone, Eq, Hash, PartialEq)]
pub enum Target {
    Background,
    Normal,
    HighPriority,
}

pub struct BlockcypherBlockchainProvider {
    host: String,
    client: reqwest::blocking::Client,
    async_client: reqwest::Client,
    network: Network,
    fees: Arc<HashMap<Target, AtomicU32>>,
}

impl BlockcypherBlockchainProvider {
    pub fn new(host: String, network: Network) -> Self {
        let host = format_host_url_with_network_path(host, network);
        let mut fees: HashMap<Target, AtomicU32> = HashMap::new();
        fees.insert(Target::Background, AtomicU32::new(MIN_FEERATE));
        fees.insert(Target::Normal, AtomicU32::new(2000));
        fees.insert(Target::HighPriority, AtomicU32::new(5000));
        let fees = Arc::new(fees);
        poll_for_fee_estimates(fees.clone(), &host);
        Self {
            host,
            network,
            client: reqwest::blocking::Client::new(),
            async_client: reqwest::Client::new(),
            fees,
        }
    }

    fn get(&self, sub_url: &str) -> Result<Response, Error> {
        self.client
            .get(format!("{}{}", self.host, sub_url))
            .send()
            .map_err(|x| {
                dlc_manager::error::Error::IOError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    x,
                ))
            })
    }

    async fn get_async(&self, sub_url: &str) -> Result<reqwest::Response, reqwest::Error> {
        self.async_client
            .get(format!("{}{}", self.host, sub_url))
            .send()
            .await
    }

    fn get_text(&self, sub_url: &str) -> Result<String, Error> {
        self.get(sub_url)?.text().map_err(|x| {
            dlc_manager::error::Error::IOError(std::io::Error::new(std::io::ErrorKind::Other, x))
        })
    }

    fn get_u64(&self, sub_url: &str) -> Result<u64, Error> {
        self.get_text(sub_url)?
            .parse()
            .map_err(|_| Error::BlockchainError)
    }

    fn get_bytes(&self, sub_url: &str) -> Result<Vec<u8>, Error> {
        let bytes = self.get(sub_url)?.bytes();
        Ok(bytes
            .map_err(|_| Error::BlockchainError)?
            .into_iter()
            .collect::<Vec<_>>())
    }

    fn get_from_json<T>(&self, sub_url: &str) -> Result<T, Error>
    where
        T: serde::de::DeserializeOwned,
    {
        self.get(sub_url)?
            .json::<T>()
            .map_err(|e| Error::OracleError(e.to_string()))
    }

    pub fn get_outspends(&self, txid: &Txid) -> Result<Vec<OutSpendResp>, Error> {
        self.get_from_json(&format!("tx/{txid}/outspends"))
    }
}

impl Blockchain for BlockcypherBlockchainProvider {
    fn send_transaction(&self, transaction: &Transaction) -> Result<(), dlc_manager::error::Error> {
        let res = self
            .client
            .post(format!("{}tx", self.host))
            .body(tx_to_string(transaction))
            .send()
            .map_err(|x| {
                dlc_manager::error::Error::IOError(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    x,
                ))
            })?;
        if let Err(error) = res.error_for_status_ref() {
            let body = res.text().unwrap_or_default();
            return Err(dlc_manager::error::Error::InvalidParameters(format!(
                "Server returned error: {error} {body}"
            )));
        }
        Ok(())
    }

    fn get_network(
        &self,
    ) -> Result<bitcoin::network::constants::Network, dlc_manager::error::Error> {
        Ok(self.network)
    }

    // This has been updated for Blockcypher
    fn get_blockchain_height(&self) -> Result<u64, dlc_manager::error::Error> {
        let block_chain_info: BlockchainInfo = self.get_from_json(&block_path())?;
        Ok(block_chain_info.height)
    }

    // This is in the process of being updated for Blockcypher
    fn get_block_at_height(&self, height: u64) -> Result<Block, dlc_manager::error::Error> {
        let block = self.get_from_json::<BlockcypherBlockInfo>(&block_height_path(height));
        if block.is_ok() {
            let block = block.unwrap();
            let time = DateTime::parse_from_rfc3339(&block.received_time)
                .unwrap()
                .timestamp() as u32;
            return Ok(Block {
                header: BlockHeader {
                    version: block.ver,
                    prev_blockhash: block.prev_block,
                    merkle_root: block.mrkl_root,
                    time,
                    bits: block.bits,
                    nonce: block.nonce,
                },
                txdata: block.txids,
            });
        } else {
            return Err(block.err().unwrap());
        };
    }

    fn get_transaction(&self, tx_id: &Txid) -> Result<Transaction, dlc_manager::error::Error> {
        let raw_tx = self.get_bytes(&format!("tx/{tx_id}/raw"))?;
        Transaction::consensus_decode(&mut std::io::Cursor::new(&*raw_tx))
            .map_err(|_| Error::BlockchainError)
    }

    fn get_transaction_confirmations(
        &self,
        tx_id: &Txid,
    ) -> Result<u32, dlc_manager::error::Error> {
        let tx_status = self.get_from_json::<TxStatus>(&format!("tx/{tx_id}/status"))?;
        if tx_status.confirmed {
            let block_chain_height = self.get_blockchain_height()?;
            if let Some(block_height) = tx_status.block_height {
                return Ok((block_chain_height - block_height + 1) as u32);
            }
        }

        Ok(0)
    }
}

impl simple_wallet::WalletBlockchainProvider for BlockcypherBlockchainProvider {
    fn get_utxos_for_address(&self, address: &bitcoin::Address) -> Result<Vec<Utxo>, Error> {
        let utxos: Vec<UtxoResp> = self.get_from_json(&format!("address/{address}/utxo"))?;

        utxos
            .into_iter()
            .map(|x| {
                Ok(Utxo {
                    address: address.clone(),
                    outpoint: OutPoint {
                        txid: x.txid.parse().map_err(|_| Error::BlockchainError)?,
                        vout: x.vout,
                    },
                    redeem_script: Script::default(),
                    reserved: false,
                    tx_out: TxOut {
                        value: x.value,
                        script_pubkey: address.script_pubkey(),
                    },
                })
            })
            .collect::<Result<Vec<_>, Error>>()
    }

    fn is_output_spent(&self, txid: &Txid, vout: u32) -> Result<bool, Error> {
        let is_spent: SpentResp = self.get_from_json(&format!("tx/{txid}/outspend/{vout}"))?;
        Ok(is_spent.spent)
    }
}

impl FeeEstimator for BlockcypherBlockchainProvider {
    fn get_est_sat_per_1000_weight(&self, confirmation_target: ConfirmationTarget) -> u32 {
        let est = match confirmation_target {
            ConfirmationTarget::Background => self
                .fees
                .get(&Target::Background)
                .unwrap()
                .load(Ordering::Acquire),
            ConfirmationTarget::Normal => self
                .fees
                .get(&Target::Normal)
                .unwrap()
                .load(Ordering::Acquire),
            ConfirmationTarget::HighPriority => self
                .fees
                .get(&Target::HighPriority)
                .unwrap()
                .load(Ordering::Acquire),
        };
        u32::max(est, MIN_FEERATE)
    }
}

impl BlockSource for BlockcypherBlockchainProvider {
    fn get_header<'a>(
        &'a self,
        header_hash: &'a bitcoin::BlockHash,
        _: Option<u32>,
    ) -> lightning_block_sync::AsyncBlockSourceResult<'a, lightning_block_sync::BlockHeaderData>
    {
        Box::pin(async move {
            let block_info: BlockInfo = self
                .get_async(&format!("block/{header_hash:x}"))
                .await
                .map_err(BlockSourceError::transient)?
                .json()
                .await
                .map_err(BlockSourceError::transient)?;
            let header_hex_str = self
                .get_async(&format!("block/{header_hash:x}/header"))
                .await
                .map_err(BlockSourceError::transient)?
                .text()
                .await
                .map_err(BlockSourceError::transient)?;
            let header_hex = bitcoin_test_utils::str_to_hex(&header_hex_str);
            let header = BlockHeader::consensus_decode(&mut std::io::Cursor::new(&*header_hex))
                .expect("to have a valid header");
            header.validate_pow(&header.target()).unwrap();
            Ok(BlockHeaderData {
                header,
                height: block_info.height,
                // Blockcypher doesn't seem to make this available.
                chainwork: Uint256::from_u64(10).unwrap(),
            })
        })
    }

    fn get_block<'a>(
        &'a self,
        header_hash: &'a bitcoin::BlockHash,
    ) -> lightning_block_sync::AsyncBlockSourceResult<'a, BlockData> {
        Box::pin(async move {
            let block_raw = self
                .get_async(&format!("block/{header_hash:x}/raw"))
                .await
                .map_err(BlockSourceError::transient)?
                .bytes()
                .await
                .map_err(BlockSourceError::transient)?;
            let block = Block::consensus_decode(&mut std::io::Cursor::new(&*block_raw))
                .expect("to have a valid header");
            Ok(BlockData::FullBlock(block))
        })
    }

    fn get_best_block(
        &self,
    ) -> lightning_block_sync::AsyncBlockSourceResult<(bitcoin::BlockHash, Option<u32>)> {
        Box::pin(async move {
            let block_tip_hash: String = self
                .get_async("blocks/tip/hash")
                .await
                .map_err(BlockSourceError::transient)?
                .text()
                .await
                .map_err(BlockSourceError::transient)?;
            let block_tip_height: u32 = self
                .get_async("blocks/tip/height")
                .await
                .map_err(BlockSourceError::transient)?
                .text()
                .await
                .map_err(BlockSourceError::transient)?
                .parse()
                .map_err(BlockSourceError::transient)?;
            Ok((
                BlockHash::from_hex(&block_tip_hash).map_err(BlockSourceError::transient)?,
                Some(block_tip_height),
            ))
        })
    }
}

impl BroadcasterInterface for BlockcypherBlockchainProvider {
    fn broadcast_transaction(&self, tx: &Transaction) {
        let client = self.client.clone();
        let host = self.host.clone();
        let body = bitcoin_test_utils::tx_to_string(tx);
        std::thread::spawn(move || {
            match client.post(format!("{host}tx")).body(body).send() {
                Err(_) => {}
                Ok(res) => {
                    if res.error_for_status_ref().is_err() {
                        // let body = res.text().unwrap_or_default();
                        // TODO(tibo): log
                    }
                }
            };
        });
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct TxStatus {
    confirmed: bool,
    block_height: Option<u64>,
    block_hash: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct UtxoResp {
    txid: String,
    vout: u32,
    value: u64,
    status: UtxoStatus,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum UtxoStatus {
    Confirmed {
        confirmed: bool,
        block_height: u64,
        block_hash: String,
        block_time: u64,
    },
    Unconfirmed {
        confirmed: bool,
    },
}

#[derive(Serialize, Deserialize, Debug)]
struct SpentResp {
    spent: bool,
}

// API Paths
fn block_path() -> String {
    "".to_string()
}

fn block_height_path(height: u64) -> String {
    format!("blocks/{height}")
}

fn store_estimate_for_target(
    fees: &Arc<HashMap<Target, AtomicU32>>,
    fee_estimates: &BlockchainInfo,
    target: Target,
) {
    #[allow(clippy::redundant_clone)]
    let val = get_estimate_for_target(fee_estimates, target.clone());
    fees.get(&target)
        .unwrap()
        .store(val, std::sync::atomic::Ordering::Relaxed);
}

fn get_estimate_for_target(fee_estimates: &BlockchainInfo, target: Target) -> u32 {
    match target {
        Target::Background => fee_estimates.low_fee_per_kb,
        Target::Normal => fee_estimates.medium_fee_per_kb,
        Target::HighPriority => fee_estimates.high_fee_per_kb,
    }
}

fn format_host_url_with_network_path(host: String, network: Network) -> String {
    match host.chars().last() {
        Some('/') => format!("{}{}", host, get_network_url(network)),
        _ => format!("{}/{}", host, get_network_url(network)),
    }
}

fn get_network_url(network: Network) -> String {
    let network_url: &str = match network {
        Network::Testnet => "v1/btc/test3/",
        Network::Bitcoin => "v1/btc/main/",
        Network::Regtest => "v1/byc/test/",
        Network::Signet => todo!(),
    };
    return network_url.to_string();
}

// This has been updated for Blockcypher
fn poll_for_fee_estimates(fees: Arc<HashMap<Target, AtomicU32>>, host: &str) {
    let host = host.to_owned();
    std::thread::spawn(move || loop {
        if let Ok(res) = reqwest::blocking::get(format!("{host}")) {
            if let Ok(blockchain_info) = res.json::<BlockchainInfo>() {
                store_estimate_for_target(&fees, &blockchain_info, Target::Background);
                store_estimate_for_target(&fees, &blockchain_info, Target::HighPriority);
                store_estimate_for_target(&fees, &blockchain_info, Target::Normal);
            }
        }

        std::thread::sleep(Duration::from_secs(60));
    });
}

#[derive(Serialize, Deserialize, Debug)]
struct BlockcypherBlockInfo {
    hash: String,
    height: u64,
    chain: String,
    total: u32,
    fees: u32,
    size: u32,
    ver: i32,
    time: String,
    received_time: String,
    coinbase_addr: String,
    relayed_by: String,
    bits: u32,
    nonce: u32,
    n_tx: u32,
    prev_block: BlockHash,
    mrkl_root: TxMerkleNode,
    txids: Vec<Transaction>,
    depth: u32,
    prev_block_url: String,
    tx_url: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct BlockchainInfo {
    name: String,
    height: u64,
    high_fee_per_kb: u32,
    medium_fee_per_kb: u32,
    low_fee_per_kb: u32,
}

#[derive(Serialize, Deserialize, Debug)]
struct BlockInfo {
    height: u32,
}

// fn sats_per_vbyte_to_sats_per_1000_weight(input: f32) -> u32 {
//     (input * 1000.0 / 4.0).round() as u32
// }

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum OutSpendResp {
    Spent(OutSpendInfo),
    Unspent { spent: bool },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct OutSpendInfo {
    pub spent: bool,
    pub txid: Txid,
    pub vin: usize,
    pub status: UtxoStatus,
}
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct Joe {
    pub name: String,
}

#[cfg(test)]
mod tests {
    extern crate mockito;
    use bitcoin::consensus::deserialize;

    use self::mockito::{mock, Mock};
    use super::*;

    fn block_height_mock(path: &str) -> Mock {
        println!("Mocking at path {path}");
        mock("GET", path)
            .with_status(200)
            // .with_header("content-type", "application/json")
            .with_body(r#"{
                "hash": "0000000000000000189bba3564a63772107b5673c940c16f12662b3e8546b412",
                "height": 294322,
                "chain": "BTC.main",
                "total": 1146652915,
                "fees": 130999,
                "size": 7787,
                "ver": 2,
                "time": "2014-04-05T07:49:18Z",
                "received_time": "2014-04-05T07:49:18Z",
                "coinbase_addr": "",
                "relayed_by": "",
                "bits": 419486617,
                "nonce": 1225187768,
                "n_tx": 10,
                "prev_block": "0000000000000000ced0958bd27720b71d32c5847e40660aaca39f33c298abb0",
                "mrkl_root": "359d624d37aee1efa5662b7f5dbc390e996d561afc8148e8d716cf6ad765a952",
                "txids": [
                    "32b3b86e40d996b1f281e24e8d4af2ceacbf874c4038369cc21baa807409b277",
                    "1579f716359ba1a207f70248135f5e5fadf539be1dcf5300613aedcb6577d287",
                    "dd1f183348eb41eaaa9ecf8012f9cca3ecbae41a6349f0cc4bfd2b1a497fa3d0",
                    "749d12ccd180968b82aef4c271ca4effdf981d9b5d12523264457c9d4e6fa78e",
                    "c4fe2ee16b8e3067d3d95caf7944011f4959781288b807df8bf853b7f80ed97c",
                    "5a2114675265522d2b7ce8a7874cfa7a22ccc3fb6566a8599d6432c6805b1b5f",
                    "077d851c8240671de80caa8be9f5285201c08a70edc5a45a9cd35fe7eaebf5e1",
                    "6202cc55fbd9130e065c9294a5b2e061c26f3d2c8df56c32da605d9f183103f9",
                    "ad3e7aa1c33f1d3e1c105d94f7b1542808da07bbe66b9621b050104a85dbf650",
                    "36cc61016b9d1bd69768666f287db1edaa9b292fb442f152af7099305677230e"
                ],
                "depth": 360235,
                "prev_block_url": "https://api.blockcypher.com/v1/btc/main/blocks/0000000000000000ced0958bd27720b71d32c5847e40660aaca39f33c298abb0",
                "tx_url": "https://api.blockcypher.com/v1/btc/main/txs/"
            }"#)
            .create()
    }

    #[test]
    fn get_block_height_test() {
        let network = bitcoin::Network::Testnet;
        let url = &mockito::server_url();
        let _block_mock = block_height_mock(&format!(
            "/{}{}",
            get_network_url(network),
            block_height_path(123)
        ));

        let block_hex = Vec::from_hex("010000004ddccd549d28f385ab457e98d1b11ce80bfea2c5ab93015ade4973e400000000bf4473e53794beae34e64fccc471dace6ae544180816f89591894e0f417a914cd74d6e49ffff001d323b3a7b0201000000010000000000000000000000000000000000000000000000000000000000000000ffffffff0804ffff001d026e04ffffffff0100f2052a0100000043410446ef0102d1ec5240f0d061a4246c1bdef63fc3dbab7733052fbbf0ecd8f41fc26bf049ebb4f9527f374280259e7cfa99c48b0e3f39c51347a19a5819651503a5ac00000000010000000321f75f3139a013f50f315b23b0c9a2b6eac31e2bec98e5891c924664889942260000000049483045022100cb2c6b346a978ab8c61b18b5e9397755cbd17d6eb2fe0083ef32e067fa6c785a02206ce44e613f31d9a6b0517e46f3db1576e9812cc98d159bfdaf759a5014081b5c01ffffffff79cda0945903627c3da1f85fc95d0b8ee3e76ae0cfdc9a65d09744b1f8fc85430000000049483045022047957cdd957cfd0becd642f6b84d82f49b6cb4c51a91f49246908af7c3cfdf4a022100e96b46621f1bffcf5ea5982f88cef651e9354f5791602369bf5a82a6cd61a62501fffffffffe09f5fe3ffbf5ee97a54eb5e5069e9da6b4856ee86fc52938c2f979b0f38e82000000004847304402204165be9a4cbab8049e1af9723b96199bfd3e85f44c6b4c0177e3962686b26073022028f638da23fc003760861ad481ead4099312c60030d4cb57820ce4d33812a5ce01ffffffff01009d966b01000000434104ea1feff861b51fe3f5f8a3b12d0f4712db80e919548a80839fc47c6a21e66d957e9c5d8cd108c7a2d2324bad71f9904ac0ae7336507d785b17a2c115e427a32fac00000000").unwrap();
        let expected_block: Block = deserialize(&block_hex).unwrap();

        let blockcypher = Arc::new(BlockcypherBlockchainProvider::new(
            url.to_string(),
            bitcoin::Network::Testnet,
        ));

        let received_block = blockcypher.get_block_at_height(123).unwrap();

        assert_eq!(expected_block, received_block);
    }
}
