import dotenv from "dotenv";
dotenv.config();

const env = process.env.ENV || "devnet";

const devnet = {
  testWalletPrivateKey:
    "b5984262748203b2043923dd34202d1a6e05601af0c00e232d3b1988ce9608f5",
  testWalletAddress: "bcrt1qpnuck30uakpc0ffcmd3nwdd59y547qlzsmf34l",
  bitcoinNetwork: "regtest",
  bitcoinNetworkURL: "http://96.126.107.204:3002",
  // TODO: which wallet?
  protocolWalletURL: "http://96.126.107.204:8085",
  attestorList: [
    "http://96.126.107.204:8801",
    "http://96.126.107.204:8802",
    "http://96.126.107.204:8803",
  ],
};

const testnet = {
  //  TODO: privatekey on testnet?
  testWalletPrivateKey:
    "bea4ecfec5cfa1e965ee1b3465ca4deff4f04b36a1fb5286a07660d5158789fb",
  testWalletAddress: "tb1q3tj2fr9scwmcw3rq5m6jslva65f2rqjxt2t0zh",
  bitcoinNetwork: "testnet",
  bitcoinNetworkURL: "https://testnet.dlc.link/electrs",
  // TODO: which wallet?
  protocolWalletURL: "https://testnet.dlc.link/stacks-wallet",
  attestorList: [
    "https://testnet.dlc.link/attestor-1",
    "https://testnet.dlc.link/attestor-2",
    "https://testnet.dlc.link/attestor-3",
  ],
};

// Local services, but regtest bitcoin
const local = {
  testWalletPrivateKey:
    "b5984262748203b2043923dd34202d1a6e05601af0c00e232d3b1988ce9608f5",
  testWalletAddress: "bcrt1qpnuck30uakpc0ffcmd3nwdd59y547qlzsmf34l",
  bitcoinNetwork: "regtest",
  bitcoinNetworkURL: "http://96.126.107.204:3002",
  protocolWalletURL: "http://127.0.0.1:8085",
  attestorList: [
    "http://localhost:8801",
    "http://localhost:8802",
    "http://localhost:8803",
  ],
};

const docker = {
  testWalletPrivateKey:
    "b5984262748203b2043923dd34202d1a6e05601af0c00e232d3b1988ce9608f5",
  testWalletAddress: "bcrt1qpnuck30uakpc0ffcmd3nwdd59y547qlzsmf34l",
  bitcoinNetwork: "regtest",
  bitcoinNetworkURL: "http://96.126.107.204:3002",
  protocolWalletURL: "http://172.17.0.1:8085",
  attestorList: [
    "http://172.17.0.1:8801",
    "http://172.17.0.1:8802",
    "http://172.17.0.1:8803",
  ],
};

const custom = {
  testWalletPrivateKey: devnet.testWalletPrivateKey,
  testWalletAddress: devnet.testWalletAddress,
  bitcoinNetwork: local.bitcoinNetwork,
  bitcoinNetworkURL: devnet.bitcoinNetworkURL,
  protocolWalletURL: local.protocolWalletURL,
  attestorList: devnet.attestorList,
};

const config = {
  devnet,
  testnet,
  local,
  docker,
  custom,
};

export default config[env];
