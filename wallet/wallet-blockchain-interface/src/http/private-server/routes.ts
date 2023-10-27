import express from 'express';
import BlockchainWriterService from '../../services/blockchain-writer.service.js';
import { localhostOnly } from '../middlewares.js';

const blockchainWriter = await BlockchainWriterService.getBlockchainWriter();
const router = express.Router();

router.post('/set-status-funded', express.json(), localhostOnly, async (req, res) => {
    console.log('POST /set-status-funded with UUID:', req.body.uuid);
    if (!req.body.uuid) {
        res.status(400).send('Missing UUID');
        return;
    }
    const data = await blockchainWriter.setStatusFunded(req.body.uuid as string);
    res.status(200).send(data);
});

router.get('/get-all-attestors', express.json(), localhostOnly, async (req, res) => {
    console.log('GET /get-all-attestors');
    let data;
    if (process.env.TEST_MODE_ENABLED === 'true') {
        data = ['http://172.20.128.5:8801', 'http://172.20.128.6:8802', 'http://172.20.128.7:8803'];
    } else {
        data = await blockchainWriter.getAllAttestors();
    }
    res.status(200).send(data);
});

router.post('/post-close-dlc', express.json(), localhostOnly, async (req, res) => {
    if (!req.body.uuid) {
        res.status(400).send('Missing UUID');
        return;
    }
    if (!req.body.btcTxId) {
        res.status(400).send('Missing BTC TX ID');
        return;
    }
    const { uuid, btcTxId } = req.body;
    console.log('POST /post-close-dlc with UUID, BTC TX ID:', uuid, btcTxId);
    const data = await blockchainWriter.postCloseDLC(uuid as string, btcTxId as string);
    res.status(200).send(data);
});

export default router;
