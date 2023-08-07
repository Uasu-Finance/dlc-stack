import { ethers } from 'ethers';
import loadNetworkConfig from './get-config.js';
import { DlcManagerV0 } from './contracts/dlc-manager-v0.js';
import { DlcManagerV1 } from './contracts/dlc-manager-v1.js';
export const getEthObserver = async (config) => {
    const networkConfig = await loadNetworkConfig(config);
    if (!networkConfig)
        throw new Error(`Could not load config for ${config.chain}.`);
    console.log(`\n[${config.chain}] Loaded config:`);
    console.dir(networkConfig.deploymentInfo, { depth: 1 });
    console.dir(networkConfig.provider, { depth: 1 });
    const deploymentInfo = networkConfig.deploymentInfo;
    const contract = new ethers.Contract(deploymentInfo.contract.address, deploymentInfo.contract.abi, networkConfig.provider);
    switch (config.version) {
        case '0':
            return DlcManagerV0(contract, deploymentInfo);
        case '1':
            return DlcManagerV1(contract, deploymentInfo);
        default:
            throw new Error(`Version ${config.version} is not supported.`);
    }
};
