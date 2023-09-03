import { JsDLCInterface } from 'dlc_protocol_wallet';
// import { off } from 'npm';

const testWalletPrivateKey = 'bea4ecfec5cfa1e965ee1b3465ca4deff4f04b36a1fb5286a07660d5158789fb';
const testWalletAddress = 'bcrt1q3tj2fr9scwmcw3rq5m6jslva65f2rqjxfrjz47';
// Pubkey: 03ab37f5b606931d7828855affe75199d952bc6174b4a23861b7ac94132210508c
// const testWalletAddress = 'tb1q3tj2fr9scwmcw3rq5m6jslva65f2rqjxt2t0zh';

const bitcoinNetwork = 'regtest';
const bitcoinNetworkURL = 'https://devnet.dlc.link/electrs';

const protocolWalletURL = 'http://localhost:8085';

const attestorList = ['https://devnet.dlc.link/attestor-1', 'https://devnet.dlc.link/attestor-2', 'https://devnet.dlc.link/attestor-3'];

const handleAttestors = false;
const successfulAttesting = false;

const testUUID = '0x9b5433920e0a7cdab5d52040ff21867597eda3c47247f6a80298ccc54510ce64';

function createMaturationDate() {
  const maturationDate = new Date();
  maturationDate.setMinutes(maturationDate.getMinutes() + 1);
  return maturationDate.toISOString();
}

async function createEvent(attestorURL, uuid) {
  const maturationDate = createMaturationDate();
  try {
    const url = `${attestorURL}/create-announcement/${uuid}`;
    console.log("Creating event at: ", url);
    const response = await fetch(url);
    const event = await response.json();
    return event;
  } catch (error) {
    console.error("Error creating event: ", error);
    process.exit(1);
  }
}

async function attest(attestorURL, uuid, outcome) {
  const response = await fetch(`${attestorURL}/v1/attest/${uuid}?outcome=${outcome}`);
  const event = await response.json();
  return event;
}

async function fetchOfferFromProtocolWallet() {
  let body = {
    uuid: testUUID,
    acceptCollateral: 10000,
    offerCollateral: 0,
    totalOutcomes: 100,
    attestorList: JSON.stringify(attestorList),
  };

  return fetch(`${protocolWalletURL}/offer`, {
    method: 'post',
    body: JSON.stringify(body),
    headers: { 'Content-Type': 'application/json' },
  }).then((res) => res.json());
}

async function sendAcceptedOfferToProtocolWallet(accepted_offer) {
  return fetch(`${protocolWalletURL}/offer/accept`, {
    method: 'put',
    body: JSON.stringify({
      acceptMessage: accepted_offer,
    }),
    headers: { 'Content-Type': 'application/json' },
  }).then((res) => res.json());
}

async function waitForBalance(dlcManager) {
  let balance = 0;
  while (balance <= 0) {
    balance = await dlcManager.get_wallet_balance();
    console.log('DLC Wasm Wallet Balance: ' + balance);
    await new Promise((resolve) => setTimeout(resolve, 5000));
  }
}

async function runDLCFlow(dlcManager, dlcOffer) {
  console.log('Starting DLC flow');

  const acceptedContract = await dlcManager.accept_offer(JSON.stringify(dlcOffer));
  const pared_response = JSON.parse(acceptedContract);
  if (!pared_response.protocolVersion) {
    console.log('Error accepting offer: ', pared_response);
    return;
  }
  console.log('Accepted Contract:', acceptedContract);

  const signedContract = await sendAcceptedOfferToProtocolWallet(acceptedContract);
  console.log('Signed Contract: ', signedContract);

  const txID = await dlcManager.countersign_and_broadcast(JSON.stringify(signedContract));
  console.log(`Broadcast funding transaction with TX ID: ${txID}`);

  if (handleAttestors) {
    console.log('Attesting to Events');
    const attestations = await Promise.all(
      exampleAttestorURLs.map((attestorURL, index) =>
        attest(attestorURL, testUUID, successful ? 100 : index === 0 ? 0 : 100)
      )
    );
    console.log('Attestation received: ', attestations);
  }

  const contracts = await dlcManager.get_contracts();
  console.log('Contracts: ', contracts);
}

async function main() {
  console.log('DLC WASM Wallet Test');

  if (handleAttestors) {
    console.log("Creating Events");
    const events = await Promise.all(
      attestorList.map((attestorURL) => createEvent(attestorURL, testUUID))
    );
    console.log("Created Events: ", events);
  }
  console.log('Fetching Offer from Protocol Wallet');
  const offerResponse = await fetchOfferFromProtocolWallet();
  if (!offerResponse.temporaryContractId) {
    console.log('Error fetching offer from protocol wallet: ', offerResponse);
    return;
  }
  console.log('Received Offer (JSON): ', offerResponse);

  // creates a new instance of the JsDLCInterface
  const dlcManager = await JsDLCInterface.new(
    testWalletPrivateKey,
    testWalletAddress,
    bitcoinNetwork,
    bitcoinNetworkURL,
    JSON.stringify(attestorList)
  );

  console.log('DLC Manager Interface Options: ', dlcManager.get_options());

  waitForBalance(dlcManager).then(() => {
    runDLCFlow(dlcManager, offerResponse);
  });
}

main();
