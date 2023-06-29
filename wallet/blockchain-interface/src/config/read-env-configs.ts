import dotenv from 'dotenv';
import { Chain, ConfigSet, validChains } from './models.js';

dotenv.config();

export default () => {
    let chain = process.env.CHAIN as Chain;
    let version = process.env.VERSION;
    let privateKey = process.env.PRIVATE_KEY;

    return {
        chain: chain,
        version: version,
        privateKey: privateKey,
    };

    // let configSets: ConfigSet[] = [];

    // for (let i = 1; ; i++) {
    //     let chain = process.env[`CHAIN` as keyof NodeJS.ProcessEnv] as Chain;
    //     let version = process.env[`VERSION` as keyof NodeJS.ProcessEnv];
    //     let privateKey = process.env[`PRIVATE_KEY` as keyof NodeJS.ProcessEnv];

    //     // Break the loop if we reach a set of variables that is not defined
    //     if (!chain && !version && !privateKey) {
    //         break;
    //     }

    //     // Throw an error if one of the set is missing
    //     if (!chain || !version) {
    //         throw new Error(`CHAIN_${i} or VERSION_${i} is missing.`);
    //     }

    //     // Throw an error if the chain is not one of the predetermined set
    //     if (!validChains.includes(chain)) {
    //         throw new Error(`CHAIN_${i}: ${chain} is not a valid chain.\nValid chains are: ${validChains.join(', ')}`);
    //     }

    //     configSets.push({
    //         chain: chain,
    //         version: version,
    //         privateKey: privateKey,
    //     });
    // }
    // return configSets;
};
