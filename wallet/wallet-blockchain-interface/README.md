# wallet-blockchain-interface

This module is responsible for communicating between the `dlc-manager` contracts on the smart-chain side, and a `router-wallet`.
It provides an API for creating and managing the DLCs of a given `router-wallet`, described in the `wallet` module of this repository [here](../wallet/).

## How it works

The wallet-blockchain-interface ("WBI") exposes two APIs:

-   a private API, used by the `router-wallet` to communicate with a blockchain
-   a public API, used to communicate with the `wbi` and the `router-wallet` from the outside

### Public API

-   GET `/health` - a simple health check endpoint
-   GET `/wallet-health` - a simple health check endpoint for the `router-wallet`
-   GET `/info` - `router-wallet` info endpoint
-   POST `/offer` - requests a new DLC offer from the `router-wallet` (TODO: add details/params)
-   PUT `/offer/accept` - accepts a given DLC offer

See the endpoint definitions [here](./src/http/public-server/routes.ts).

### Private API

The app is exposing three endpoints for the `router-wallet` to communicate with:

-   POST `/set-status-funded` - sets the status of a given DLC to `funded`
-   GET `/get-all-attestors` - returns all the attestors that are currently registered on the smart contract
-   POST `/post-close-dlc` - used as a callback after successful DLC attestation

See the endpoint definitions [here](./src/http/private-server/routes.ts).

Depending on the environment setup, it communicates with different Blockchains (one at a time). Take a look at the [`blockchain-writer.service.ts`](./src/services/blockchain-writer.service.ts) file to see how it works.

## How to run

### 1. As a standalone NodeJs application

> Create a `.env` file in this folder, and set the variables there, based on the `.env.template` file.

In this folder, run:

```bash
npm install
npm run build
npm run start
# or for a dev build
npm run dev
```

### 2. Docker Compose through `make`

In the root of the repository, run:

```bash
make docker-start
```

This will build and start all the services at once, including an instance of this `blockchain-interface`.

### 3. justfile

In the root of the `wallet` module, run:

```bash
just run
```

This will build and run both the `router-wallet` and the `wbi` at once.
