import { Attestor } from 'attestor';
import { getEnv } from '../config/read-env-configs.js';
import { createECDH } from 'crypto';
import { readFileSync, writeFileSync, existsSync } from 'fs';
async function getOrGenerateSecretFromConfig(secretKeyFile) {
    let secretKeyPath = secretKeyFile;
    let secretKey;
    if (existsSync(secretKeyPath)) {
        console.log(`Reading secret key from ${secretKeyPath}`);
        secretKey = readFileSync(secretKeyPath, { encoding: 'utf8' }).trim();
    }
    else {
        console.log('No secret key file was found, generating secret key');
        const ecdh = createECDH('secp256k1');
        ecdh.generateKeys();
        secretKey = ecdh.getPrivateKey('hex');
        writeFileSync(secretKeyPath, secretKey);
    }
    return secretKey;
}
function createMaturationDate() {
    const maturationDate = new Date();
    maturationDate.setMinutes(maturationDate.getMinutes() + 3);
    return maturationDate.toISOString();
}
export default class AttestorService {
    static attestor;
    constructor() { }
    static async getAttestor() {
        if (!this.attestor) {
            this.attestor = await Attestor.new(getEnv('STORAGE_API_ENABLED') === 'true', getEnv('STORAGE_API_ENDPOINT'), await getOrGenerateSecretFromConfig(`../config/${getEnv('SECRET_KEY_FILE')}`));
            console.log('Attestor created');
        }
        console.log('Attestor public key:', await this.attestor.get_pubkey());
        return this.attestor;
    }
    static async init() {
        await this.getAttestor();
    }
    static async createAnnouncement(uuid, maturation) {
        const attestor = await this.getAttestor();
        let _maturation = maturation ? new Date(maturation).toISOString() : createMaturationDate();
        await attestor.create_event(uuid, _maturation);
    }
    static async createAttestation(uuid, value, precisionShift = 0) {
        const attestor = await this.getAttestor();
        const formatOutcome = (value) => BigInt(Math.round(value / 10 ** precisionShift));
        // We can safely assume that the value is not bigger than 2^53 - 1
        const formattedOutcome = formatOutcome(Number(value));
        await attestor.attest(uuid, formattedOutcome);
    }
    static async getEvent(uuid) {
        const attestor = await this.getAttestor();
        try {
            const event = await attestor.get_event(uuid);
            return event;
        }
        catch (error) {
            console.error(error);
            return null;
        }
    }
    static async getAllEvents() {
        const attestor = await this.getAttestor();
        try {
            const events = await attestor.get_events();
            return events;
        }
        catch (error) {
            console.error(error);
            return null;
        }
    }
    static async getPublicKey() {
        const attestor = await this.getAttestor();
        try {
            const publicKey = await attestor.get_pubkey();
            return publicKey;
        }
        catch (error) {
            console.error(error);
            return null;
        }
    }
}
