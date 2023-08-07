import dotenv from 'dotenv';
import { validChains } from './models.js';
dotenv.config();
export function getEnv(key) {
    const value = process.env[key];
    if (!value)
        throw new Error(`Environment variable ${key} is missing.`);
    return value;
}
export default () => {
    let configSets = [];
    const configRegEx = /^(CHAIN|VERSION|API_KEY)_(\d+)$/;
    let tempConfigs = {};
    for (let key in process.env) {
        let match = configRegEx.exec(key);
        if (match) {
            let variableName = match[1];
            let configNumber = match[2];
            if (!(configNumber in tempConfigs)) {
                tempConfigs[configNumber] = {
                    chain: '',
                    version: '',
                    api_key: '',
                };
            }
            tempConfigs[configNumber][variableName.toLowerCase()] = process.env[key];
        }
    }
    for (let configNumber in tempConfigs) {
        let { chain, version, api_key } = tempConfigs[configNumber];
        chain = chain;
        if (!chain || !version) {
            throw new Error(`CHAIN_${configNumber} or VERSION_${configNumber} is missing.`);
        }
        if (!validChains.includes(chain)) {
            throw new Error(`CHAIN_${configNumber}: ${chain} is not a valid chain.\nValid chains are: ${validChains.join(', ')}`);
        }
        configSets.push({
            chain: chain,
            version: version,
            api_key: api_key,
        });
    }
    return configSets;
};
