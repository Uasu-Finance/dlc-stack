import AttestorService from '../../../services/attestor.service.js';
export const DlcManagerV1 = (contract, deploymentInfo) => {
    return {
        start: () => {
            contract.on('CreateDLC', async (_uuid, _attestorList, _creator, _protocolWallet, _eventSource, tx) => {
                const currentTime = new Date();
                const _logMessage = `[${deploymentInfo.network}][${deploymentInfo.contract.name}] New DLC Request... @ ${currentTime} \n\t uuid: ${_uuid} | creator: ${_creator} | attestors: ${_attestorList} \n`;
                console.log(_logMessage);
                console.log('TXID:', tx.transactionHash);
                try {
                    await AttestorService.createAnnouncement(_uuid);
                    console.log(await AttestorService.getEvent(_uuid));
                }
                catch (error) {
                    console.error(error);
                }
            });
            contract.on('SetStatusFunded', async (_uuid, _creator, _protocolWallet, _sender, _eventSource) => {
                const currentTime = new Date();
                const _logMessage = `[${deploymentInfo.network}][${deploymentInfo.contract.name}] DLC funded @ ${currentTime} \n\t uuid: ${_uuid} | creator: ${_creator} | protocolWallet: ${_protocolWallet} | sender: ${_sender} \n`;
                console.log(_logMessage);
            });
            contract.on('CloseDLC', async (_uuid, _outcome, _creator, _protocolWallet, _sender, _eventSource) => {
                const currentTime = new Date();
                const outcome = BigInt(_outcome);
                const _logMessage = `[${deploymentInfo.network}][${deploymentInfo.contract.name}] Closing DLC... @ ${currentTime} \n\t uuid: ${_uuid} | outcome: ${outcome} \n`;
                console.log(_logMessage);
                try {
                    // NOTE: precision_shift is hardcoded to 2
                    await AttestorService.createAttestation(_uuid, outcome, 2);
                    console.log(await AttestorService.getEvent(_uuid));
                }
                catch (error) {
                    console.error(error);
                }
            });
            contract.on('PostCloseDLC', async (_uuid, _outcome, _creator, _protocolWallet, _sender, _btcTxId, _eventSource) => {
                const currentTime = new Date();
                const _logMessage = `[${deploymentInfo.network}][${deploymentInfo.contract.name}] DLC closed @ ${currentTime} \n\t uuid: ${_uuid} | outcome: ${_outcome} | btcTxId: ${_btcTxId} \n`;
                console.log(_logMessage);
            });
        },
    };
};