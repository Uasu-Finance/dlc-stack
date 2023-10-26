import express from 'express';
import readEnvConfigs from '../../config/read-env-configs.js';
import RouterWalletService from '../../services/router-wallet.service.js';

const router = express.Router();
const routerWallet = await RouterWalletService.getRouterWallet();

router.get('/health', express.json(), async (req, res) => {
    const data = readEnvConfigs();
    console.log('GET /health');
    res.status(200).send({ chain: data.chain, version: data.version });
});

router.get('/wallet-health', express.json(), async (req, res) => {
    console.log('GET /wallet-health');
    const data = await routerWallet.getHealth();
    res.status(200).send(data);
});

router.get('/info', express.json(), async (req, res) => {
    console.log('GET /info');
    const data = await routerWallet.getInfo();
    res.status(200).send(data);
});

router.post('/offer', express.json(), async (req, res) => {
    console.log('POST /offer');
    const data = await routerWallet.getOffer(req.body);
    res.status(200).send(data);
});

router.put('/offer/accept', express.json(), async (req, res) => {
    console.log('PUT /offer/accept');
    const data = await routerWallet.acceptOffer(req.body);
    res.status(200).send(data);
});

export default router;
