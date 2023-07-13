import { JsDLCInterface } from "dlc_protocol_wallet";

async function fetchOfferFromProtocolWallet() {
    let body = {
        "uuid": "abc12345",
        "acceptCollateral": 10000,
        "offerCollateral": 0,
        "totalOutcomes": 100
    };

    return fetch('http://localhost:8085/offer', {
        method: 'post',
        body: JSON.stringify(body),
        headers: { 'Content-Type': 'application/json' },
    }).then(res => res.json());
}

async function sendAcceptedOfferToProtocolWallet(accepted_offer) {
    return fetch('http://localhost:8085/offer/accept', {
        method: 'put',
        body: JSON.stringify({
            'acceptMessage': accepted_offer
        }),
        headers: { 'Content-Type': 'application/json' },
    }).then(res => res.json());
}

const key = "bea4ecfec5cfa1e965ee1b3465ca4deff4f04b36a1fb5286a07660d5158789fb";
const address = "tb1q3tj2fr9scwmcw3rq5m6jslva65f2rqjxt2t0zh";

async function go() {
    console.log("DLC WASM test let's go");

    // Create a new dlc manager interface
    const dlc_man = await JsDLCInterface.new(key, address, "testnet", "https://blockstream.info/testnet/api", "https://testnet.dlc.link/oracle");
    console.log("dlc manager interface options: ", dlc_man.get_options());

    var balance = 0;
    // use a setInterval to wait for the balance to be > 0
    var loopId = setInterval(() => {
        if (balance > 0) {
            clearInterval(loopId);

            runDLCFlow(dlc_man);
        }
        dlc_man.get_wallet_balance().then((bal) => {
            balance = bal;
            console.log("Balance: " + balance);
        });
    }, 5000);
}

async function runDLCFlow(dlc_man) {
    console.log("Starting DLC flow");

    const offer_json = await fetchOfferFromProtocolWallet();
    console.log("offer_json: ", offer_json);

    const accepted_contract = await dlc_man.accept_offer(JSON.stringify(offer_json))
    console.log("Got response from receive_offer_and_accept", accepted_contract);

    const signed_contract = await sendAcceptedOfferToProtocolWallet(accepted_contract);
    console.log("signed offer: ", signed_contract);

    const tx_id = await dlc_man.countersign_and_broadcast(JSON.stringify(signed_contract))
    console.log(`Broadcast DLC with tx-id ${tx_id}`);

    const contracts = await dlc_man.get_contracts();
    console.log("Got contracts: ", contracts);
}

go();
