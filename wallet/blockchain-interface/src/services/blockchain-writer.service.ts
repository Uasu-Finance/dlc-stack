import { Chain } from '../config/models.js';

export default class BlockchainWriterService {
    private static blockchainWriter: BlockchainWriterService;
    private chain: Chain;

    private constructor(chain: Chain) {
        this.chain = chain;
    }

    public static async getBlockchainWriter(chain: Chain): Promise<BlockchainWriterService> {
        if (!this.blockchainWriter) this.blockchainWriter = new BlockchainWriterService(chain);
        return this.blockchainWriter;
    }

    public getChain(): Chain {
        return this.chain;
    }

    public async setStatusFunded(uuid: string): Promise<void> {
        // Based on chain, we will call the corresponding blockchain writer
        // That in turn, will get the right contract based on version?
        switch (this.chain) {
        }
    }
}
