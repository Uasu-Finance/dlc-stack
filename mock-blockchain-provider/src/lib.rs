use bitcoin::Network;
use dlc_manager::{error::Error, Blockchain, Utxo};
use lightning::chain::chaininterface::{BroadcasterInterface, ConfirmationTarget, FeeEstimator};

pub struct MockBlockchainProvider {
    host: String,
    network: Network,
}

impl MockBlockchainProvider {
    pub fn new(host: String, network: Network) -> Self {
        Self { host, network }
    }
}

impl Blockchain for MockBlockchainProvider {
    fn send_transaction(
        &self,
        transaction: &bitcoin::Transaction,
    ) -> Result<(), dlc_manager::error::Error> {
        todo!()
    }

    fn get_network(
        &self,
    ) -> Result<bitcoin::network::constants::Network, dlc_manager::error::Error> {
        todo!()
    }

    fn get_blockchain_height(&self) -> Result<u64, dlc_manager::error::Error> {
        todo!()
    }

    fn get_block_at_height(
        &self,
        height: u64,
    ) -> Result<bitcoin::Block, dlc_manager::error::Error> {
        todo!()
    }

    fn get_transaction(
        &self,
        tx_id: &bitcoin::Txid,
    ) -> Result<bitcoin::Transaction, dlc_manager::error::Error> {
        todo!()
    }

    fn get_transaction_confirmations(
        &self,
        tx_id: &bitcoin::Txid,
    ) -> Result<u32, dlc_manager::error::Error> {
        todo!()
    }
}

impl simple_wallet::WalletBlockchainProvider for MockBlockchainProvider {
    fn get_utxos_for_address(&self, address: &bitcoin::Address) -> Result<Vec<Utxo>, Error> {
        todo!()
    }

    fn is_output_spent(&self, txid: &bitcoin::Txid, vout: u32) -> Result<bool, Error> {
        todo!()
    }
}

impl FeeEstimator for MockBlockchainProvider {
    fn get_est_sat_per_1000_weight(&self, confirmation_target: ConfirmationTarget) -> u32 {
        todo!()
    }
}

// impl BlockSource for MockBlockchainProvider {}

// impl BroadcasterInterface for MockBlockchainProvider {}
