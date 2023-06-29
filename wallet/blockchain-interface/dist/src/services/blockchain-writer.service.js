import readEnvConfigs from '../config/read-env-configs.js';
import getETHConfig from '../chains/ethereum/get-config.js';
export default class BlockchainWriterService {
    static blockchainWriter;
    constructor() { }
    static async getBlockchainWriter() {
        if (!this.blockchainWriter)
            this.blockchainWriter = new BlockchainWriterService();
        return this.blockchainWriter;
    }
    async getConfig() {
        let configSet = readEnvConfigs();
        switch (configSet.chain) {
            case 'ETH_MAINNET':
            case 'ETH_SEPOLIA':
            case 'ETH_GOERLI':
            case 'ETH_LOCAL':
                return await getETHConfig(configSet);
            case 'STACKS_MAINNET':
            case 'STACKS_TESTNET':
            case 'STACKS_MOCKNET':
            case 'STACKS_LOCAL':
            // return getStacksConfig(configSet);
            default:
                throw new Error(`${configSet.chain} is not a valid chain.`);
        }
    }
    async setStatusFunded(uuid) {
        const contractConfig = await this.getConfig();
        return await contractConfig.setStatusFunded(uuid);
    }
    async postCloseDLC(uuid, btcTxId) {
        const contractConfig = await this.getConfig();
        return await contractConfig.postCloseDLC(uuid, btcTxId);
    }
}
