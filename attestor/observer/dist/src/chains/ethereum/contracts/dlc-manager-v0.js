import AttestorService from '../../../services/attestor.service.js';
export const DlcManagerV0 = (contract, deploymentInfo) => {
    return {
        start: () => {
            contract.on('CreateDLC', async (_uuid, _creator, _receiver, _emergencyRefundTime, _nonce, _eventSource) => {
                const currentTime = new Date();
                const emergencyRefundTime = _emergencyRefundTime.toNumber().toString();
                const nonce = _nonce.toNumber().toString();
                const _logMessage = `[${deploymentInfo.network}][${deploymentInfo.contract.name}] New DLC Request... @ ${currentTime} \n\t uuid: ${_uuid} | emergencyRefundTime: ${emergencyRefundTime} | creator: ${_creator} \n`;
                console.log(_logMessage);
                try {
                    await AttestorService.createAnnouncement(_uuid);
                    console.log(await AttestorService.getEvent(_uuid));
                }
                catch (error) {
                    console.error(error);
                }
            });
            contract.on('CloseDLC', async (_uuid, _outcome, _creator, _eventSource) => {
                const currentTime = new Date();
                const outcome = _outcome.toBigInt();
                const _logMessage = `[${deploymentInfo.network}][${deploymentInfo.contract.name}] Closing DLC... @ ${currentTime} \n\t uuid: ${_uuid} | outcome: ${outcome} \n`;
                console.log(_logMessage);
                // TODO: precision_shift?
                try {
                    await AttestorService.createAttestation(_uuid, outcome);
                    console.log(await AttestorService.getEvent(_uuid));
                }
                catch (error) {
                    console.error(error);
                }
            });
        },
    };
};
