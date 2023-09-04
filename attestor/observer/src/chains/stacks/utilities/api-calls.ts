import { NFTHoldingsData } from '../models/interfaces.js';
import type { ContractCallTransaction } from '@stacks/stacks-blockchain-api-types';

export async function fetchTXInfo(txId: string, api_base_extended: string): Promise<ContractCallTransaction> {
  console.log(`[Stacks] Fetching tx_info... ${txId}`);
  try {
    const response = await fetch(api_base_extended + '/tx/' + txId);
    return (await response.json()) as ContractCallTransaction;
  } catch (err) {
    console.error(err);
    throw err;
  }
}
