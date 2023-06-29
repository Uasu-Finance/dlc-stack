import express from 'express';
// import AttestorService from '../services/attestor.service.js';
import BlockchainWriterService from '../services/blockchain-writer.service.js';
import { Chain } from '../config/models.js';
import getConfig from '../config/read-env-configs.js';

const config = getConfig();
const blockchainWriter = await BlockchainWriterService.getBlockchainWriter(config.chain);
const router = express.Router();

router.get('/health', (req, res) => {
    res.status(200).send(blockchainWriter.getChain());
});

router.get('/event', async (req, res) => {
    if (!req.query.uuid) {
        res.status(400).send('Missing UUID');
        return;
    }
    console.log('GET /event with UUID:', req.query.uuid);
    // const data = await AttestorService.getEvent(req.query.uuid as string);
    res.status(200).send('data');
});

router.get('/events', async (req, res) => {
    console.log('GET /events');
    // const data = await AttestorService.getAllEvents();
    res.status(200).send('data');
});

export default router;
