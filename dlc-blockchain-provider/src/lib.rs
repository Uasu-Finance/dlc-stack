use std::sync::Arc;

use bitcoin::{Block, Transaction, Txid};
use dlc_manager::{error::Error, Blockchain as DlcBlockchain, Utxo};

use js_interface_wallet::{JSInterfaceWallet, WalletBlockchainProvider};
use lightning::chain::chaininterface::FeeEstimator;

use serde::{Deserialize, Serialize};

use wasm_bindgen_futures::spawn_local;

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

#[derive(Serialize, Deserialize, Debug, Clone)]
struct UTXOSpent {
    spent: bool,
}

pub struct DlcBlockchainProvider {
    wallet: Arc<JSInterfaceWallet>,
}

impl DlcBlockchainProvider {
    pub fn new(wallet: Arc<JSInterfaceWallet>) -> Self {
        Self { wallet }
    }
}

impl DlcBlockchain for DlcBlockchainProvider {
    fn send_transaction(&self, transaction: &Transaction) -> Result<(), Error> {
        let x = self.wallet.client().clone();
        let y = transaction.clone();
        spawn_local(async move { x.broadcast(&y).await.unwrap() });
        Ok(())
    }

    fn get_network(&self) -> Result<bitcoin::network::constants::Network, Error> {
        Ok(self.wallet.network())
    }
    fn get_blockchain_height(&self) -> Result<u64, Error> {
        Ok(self.wallet.get_height())
    }
    fn get_block_at_height(&self, _height: u64) -> Result<Block, Error> {
        // Currently this is only used for Lightning support, so we don't need to implement it
        unimplemented!();
    }
    fn get_transaction(&self, tx_id: &Txid) -> Result<Transaction, Error> {
        match self.wallet.get_tx(tx_id).unwrap() {
            None => Err(Error::BlockchainError("tx not found".to_string())),
            Some(tx) => match tx.transaction {
                Some(tx) => Ok(tx),
                _ => Err(Error::BlockchainError("tx not found".to_string())),
            },
        }
    }
    fn get_transaction_confirmations(&self, _tx_id: &Txid) -> Result<u32, Error> {
        // Currently this is only used in the periodic_check, which is a task of the protocol wallet, so we don't need to implement it
        unimplemented!()
    }
}

impl WalletBlockchainProvider for DlcBlockchainProvider {
    fn get_utxos_for_address(&self, _address: &bitcoin::Address) -> Result<Vec<Utxo>, Error> {
        Ok(self.wallet.get_utxos().unwrap())
    }

    fn is_output_spent(&self, txid: &Txid, vout: u32) -> Result<bool, Error> {
        let utxos = self.wallet.get_utxos().unwrap();
        let matched_utxo = utxos.into_iter().find(|utxo| utxo.outpoint.txid == *txid);
        if matched_utxo.is_none() {
            return Ok(false);
        }
        let matched_utxo = matched_utxo.unwrap();
        Ok(matched_utxo.outpoint.vout == vout)
    }
}

impl FeeEstimator for DlcBlockchainProvider {
    fn get_est_sat_per_1000_weight(
        &self,
        _confirmation_target: lightning::chain::chaininterface::ConfirmationTarget,
    ) -> u32 {
        unimplemented!()
    }
}
