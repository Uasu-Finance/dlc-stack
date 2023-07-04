import fetch from 'cross-fetch';
import { ethers } from 'ethers';
import { WebSocketProvider } from './utilities/websocket-provider.js';
import fs from 'fs';
async function fetchDeploymentInfo(subchain, version) {
    // TODO: versioning the deployment files
    const contract = 'DlcManager';
    try {
        const response = await fetch(`https://raw.githubusercontent.com/DLC-link/dlc-solidity/master/deploymentFiles/${subchain}/${contract}.json`);
        return await response.json();
    }
    catch (error) {
        throw new Error(`Could not fetch deployment info for ${contract} on ${subchain}`);
    }
}
async function getLocalDeploymentInfo(path, contract, version) {
    try {
        let dp = JSON.parse(fs.readFileSync(`${path}/v${version}/${contract}.json`, 'utf-8'));
        return dp;
    }
    catch (error) {
        console.log(error);
        throw new Error(`Could not fetch deployment info for ${contract} on local`);
    }
}
export default async (config) => {
    let deploymentInfo = {};
    let provider;
    let wallet;
    switch (config.chain) {
        case 'ETH_MAINNET':
            deploymentInfo = await fetchDeploymentInfo('mainnet', config.version);
            provider = new WebSocketProvider(`wss://mainnet.infura.io/ws/v3/${config.apiKey}`);
            wallet = new ethers.Wallet(config.privateKey, provider);
            break;
        case 'ETH_SEPOLIA':
            deploymentInfo = await fetchDeploymentInfo('sepolia', config.version);
            provider = new WebSocketProvider(`wss://sepolia.infura.io/ws/v3/${config.apiKey}`);
            wallet = new ethers.Wallet(config.privateKey, provider);
            break;
        case 'ETH_GOERLI':
            deploymentInfo = await fetchDeploymentInfo('goerli', config.version);
            provider = new WebSocketProvider(`wss://goerli.infura.io/ws/v3/${config.apiKey}`);
            wallet = new ethers.Wallet(config.privateKey, provider);
            break;
        case 'ETH_LOCAL':
            deploymentInfo = await getLocalDeploymentInfo('./deploymentFiles/localhost', 'DlcManager', config.version); // TODO:
            provider = new ethers.providers.JsonRpcProvider(`http://127.0.0.1:8545`);
            wallet = new ethers.Wallet(config.privateKey, provider);
            break;
        default:
            throw new Error(`Chain ${config.chain} is not supported.`);
            break;
    }
    const contract = new ethers.Contract(deploymentInfo.contract.address, deploymentInfo.contract.abi, provider).connect(wallet);
    return {
        setStatusFunded: async (uuid) => {
            try {
                const gasLimit = await contract.estimateGas.setStatusFunded(uuid);
                const transaction = await contract.setStatusFunded(uuid, {
                    gasLimit: gasLimit.add(10000),
                });
                const txReceipt = await transaction.wait();
                console.log('Funded request transaction receipt: ', txReceipt);
                return txReceipt;
            }
            catch (error) {
                console.log(error);
                return error;
            }
        },
        postCloseDLC: async (uuid, btcTxId) => {
            try {
                const gasLimit = await contract.estimateGas.postCloseDLC(uuid, btcTxId);
                const transaction = await contract.postCloseDLC(uuid, btcTxId, {
                    gasLimit: gasLimit.add(10000),
                });
                const txReceipt = await transaction.wait();
                console.log('PostCloseDLC transaction receipt: ', txReceipt);
                return txReceipt;
            }
            catch (error) {
                console.log(error);
                return error;
            }
        },
    };
};