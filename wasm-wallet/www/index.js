import { JsDLCInterface } from "dlc_protocol_wallet";

async function fetchOffer() {
    let body = {
        "uuid": "abc12345",
        "acceptCollateral": 100000,
        "offerCollateral": 0,
        "totalOutcomes": 100
    };

    // fetch post with body
    // use fetch library to do a post request to the offeror wallet
    return fetch('http://localhost:8085/offer', {
        method: 'post',
        body: JSON.stringify(body),
        headers: { 'Content-Type': 'application/json' },
    }).then(res => res.json());
}

async function setup() {
    console.log("Setting up");
    const dlc_man = await JsDLCInterface.new();
    console.log(dlc_man.send_options_to_js());
    var balance = 0;
    var offered_contract;
    var runonce = true;
    // call doWork every 5 seconds in a loop
    var _loopId = setInterval(() => {
        dlc_man.get_wallet_balance().then((bal) => balance = bal);
        console.log("Balance: " + balance);
        if (!offered_contract && balance > 0 && runonce) {
            runonce = false;

            fetchOffer().then((offer_json) => {
                console.log("offer_json: ", offer_json);


                dlc_man.receive_offer_and_accept(JSON.stringify(offer_json)).then((accepted_contract) => {

                    console.log("Got response from receive_offer_and_accept", accepted_contract);
                    fetch('http://localhost:8085/offer/accept', {
                        method: 'put',
                        body: JSON.stringify({
                            'acceptMessage': accepted_contract
                        }),
                        headers: { 'Content-Type': 'application/json' },
                    }).then(res => {
                        console.log("intermediary response: ", res);
                        return res.json();
                    }).then((signed_contract) => {

                        console.log("signed offer: ", signed_contract);
                        dlc_man.countersign_and_broadcast(JSON.stringify(signed_contract)).then((res) => {
                            console.log("Got response from sign_offer", res);
                        });

                    });
                });
            });
        }
    }, 3000);
}

setup();
