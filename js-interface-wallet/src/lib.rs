use std::{cell::RefCell, str::FromStr};

use bdk::{
    wallet::coin_selection::{decide_change, CoinSelectionResult},
    FeeRate, KeychainKind, LocalUtxo, Utxo as BdkUtxo, WeightedUtxo,
};
use bitcoin::{hashes::Hash, Address, PrivateKey, Script, Txid};
use dlc_manager::{error::Error, Blockchain, Signer, Utxo, Wallet};
use lightning::chain::chaininterface::FeeEstimator;
use secp256k1_zkp::{All, PublicKey, Secp256k1, SecretKey};
type Result<T> = core::result::Result<T, Error>;

pub(crate) const TXIN_BASE_WEIGHT: usize = (32 + 4 + 4) * 4;

/// Trait providing blockchain information to the wallet.
pub trait WalletBlockchainProvider: Blockchain + FeeEstimator {
    fn get_utxos_for_address(&self, address: &Address) -> Result<Vec<Utxo>>;
    fn is_output_spent(&self, txid: &Txid, vout: u32) -> Result<bool>;
}

pub struct JSInterfaceWallet {
    address: Address,
    secp_ctx: Secp256k1<All>,
    seckey: SecretKey,
    utxos: RefCell<Option<Vec<Utxo>>>,
}

impl JSInterfaceWallet {
    pub fn new(address_str: String, privkey: PrivateKey) -> Self {
        Self {
            address: Address::from_str(&address_str).unwrap(),
            secp_ctx: Secp256k1::new(),
            seckey: privkey.inner,
            utxos: Some(vec![]).into(),
        }
    }

    pub fn set_utxos(&self, mut utxos: Vec<Utxo>) -> Result<()> {
        self.utxos.borrow_mut().as_mut().unwrap().clear();
        self.utxos.borrow_mut().as_mut().unwrap().append(&mut utxos);
        Ok(())
    }

    // Returns the sum of all UTXOs value.
    pub fn get_balance(&self) -> u64 {
        self.utxos
            .borrow()
            .as_ref()
            .unwrap()
            .iter()
            .map(|x| x.tx_out.value)
            .sum()
    }
}

impl Signer for JSInterfaceWallet {
    fn sign_tx_input(
        &self,
        tx: &mut bitcoin::Transaction,
        input_index: usize,
        tx_out: &bitcoin::TxOut,
        _: Option<bitcoin::Script>,
    ) -> Result<()> {
        dlc::util::sign_p2wpkh_input(
            &self.secp_ctx,
            &self.seckey,
            tx,
            input_index,
            bitcoin::EcdsaSighashType::All,
            tx_out.value,
        )?;
        Ok(())
    }

    fn get_secret_key_for_pubkey(&self, _pubkey: &PublicKey) -> Result<SecretKey> {
        Ok(self.seckey)
    }
}

fn select_sorted_utxos(
    utxos: impl Iterator<Item = (bool, WeightedUtxo)>,
    fee_rate: FeeRate,
    target_amount: u64,
    drain_script: &Script,
) -> Result<CoinSelectionResult> {
    let mut selected_amount = 0;
    let mut fee_amount = 0;
    let selected = utxos
        .scan(
            (&mut selected_amount, &mut fee_amount),
            |(selected_amount, fee_amount), (must_use, weighted_utxo)| {
                if must_use || **selected_amount < target_amount + **fee_amount {
                    **fee_amount +=
                        fee_rate.fee_wu(TXIN_BASE_WEIGHT + weighted_utxo.satisfaction_weight);
                    **selected_amount += weighted_utxo.utxo.txout().value;

                    Some(weighted_utxo.utxo)
                } else {
                    None
                }
            },
        )
        .collect::<Vec<_>>();

    let amount_needed_with_fees = target_amount + fee_amount;
    if selected_amount < amount_needed_with_fees {
        return Err(Error::InvalidParameters("Insufficient Funds".to_string()));
    }

    let remaining_amount = selected_amount - amount_needed_with_fees;

    let excess = decide_change(remaining_amount, fee_rate, drain_script);

    Ok(CoinSelectionResult {
        selected,
        fee_amount,
        excess,
    })
}

impl Wallet for JSInterfaceWallet {
    fn get_new_address(&self) -> Result<Address> {
        Ok(self.address.clone())
    }

    fn get_new_secret_key(&self) -> Result<SecretKey> {
        Ok(self.seckey)
    }

    fn get_utxos_for_amount(
        &self,
        amount: u64,
        fee_rate: Option<u64>,
        _lock_utxos: bool,
    ) -> Result<Vec<Utxo>> {
        let org_utxos = self.utxos.borrow().as_ref().unwrap().clone();
        let mut utxos = org_utxos
            .iter()
            .filter(|x| !x.reserved)
            .map(|x| WeightedUtxo {
                utxo: BdkUtxo::Local(LocalUtxo {
                    outpoint: x.outpoint,
                    txout: x.tx_out.clone(),
                    keychain: KeychainKind::External,
                    is_spent: false,
                }),
                satisfaction_weight: 107,
            })
            .collect::<Vec<_>>();
        let dummy_pubkey: PublicKey =
            "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798"
                .parse()
                .unwrap();
        let dummy_drain =
            Script::new_v0_p2wpkh(&bitcoin::WPubkeyHash::hash(&dummy_pubkey.serialize()));
        let fee_rate = FeeRate::from_sat_per_vb(fee_rate.unwrap() as f32);
        let required_utxos = Vec::new();
        let drain_script = &dummy_drain;

        let temp_utxos = {
            utxos.sort_unstable_by_key(|wu| wu.utxo.txout().value);
            required_utxos
                .into_iter()
                .map(|utxo| (true, utxo))
                .chain(utxos.into_iter().rev().map(|utxo| (false, utxo)))
        };

        let selection = select_sorted_utxos(temp_utxos, fee_rate, amount, drain_script).unwrap();

        let mut res = Vec::new();
        for utxo in selection.selected {
            let local_utxo = if let BdkUtxo::Local(l) = utxo {
                l
            } else {
                panic!();
            };
            let org = org_utxos
                .iter()
                .find(|x| x.tx_out == local_utxo.txout && x.outpoint == local_utxo.outpoint)
                .unwrap();
            res.push(org.clone());
        }
        Ok(res)
    }

    fn import_address(&self, _: &Address) -> Result<()> {
        // unimplemented!()
        Ok(())
    }
}

#[derive(Clone)]
struct UtxoWrap {
    utxo: Utxo,
}

impl rust_bitcoin_coin_selection::Utxo for UtxoWrap {
    fn get_value(&self) -> u64 {
        self.utxo.tx_out.value
    }
}
