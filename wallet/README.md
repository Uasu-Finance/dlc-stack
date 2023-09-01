# DLC.Link Router Wallet
A Bitcoin wallet which supports the creation and signing of DLCs via an HTTP server.

This application is written in Rust and is intended to compile to a binary and run directly as a service. It also has a docker file and is ready to be run as a container.

## Structure
This application currently only supports the creation of a DLC offer, and then later signing that offer. It does not support "accepting" an offer from another DLC wallet.

This application is intended to function as a service of a decentralized application (dapp). Because of this, this application never posts collateral into the DLC. It also does not pay any gas fee related to the DLC (funding tx nor any of the CET outcomes).

## Routing funds
This application is intended to be a counterparty in a DLC for a dapp, and then "route" any funds it gets to a payout bitcoin address in an automated way upon DLC closing. This can be done in two ways:

### 1. External funding address for DLCs
The routing happens as a built-in mechanism of the DLC, as this application will put a given payout address in as the payout address of the DLC (in the CETs). This way, the funds of the DLC never actually go to this application, but rather to the payout address directly. In this case, the wallet of this application never receives or needs any Bitcoin, and just acts as an automated counterparty of a DLC.

### 2. Manual funds routing
If this applications bitcoin-wallet address is used as the funding output address of the DLC, then the funds that go to this wallet will need to be manually moved to some other location, such as another wallet, or a bridge, etc.

## How to run

There are two ways to run this attestor. We have prebuilt docker images available on our AWS Container Registry, or one can pull this repository, and build & run from source.

> Important!
>
> If you wish to participate in the DLC.Link Attestation Layer, you must also register your attestor on our network. This requires a static public IP address, and the registration of this address on our smart contracts. Contact us for details.

### Option 1. Run using Docker

You will need docker installed on your machine. Fetch the preset docker-compose file:

```bash
$ wget https://github.com/DLC-link/dlc-stack/raw/master/attestor/docker-compose.yml
```

Note the environment variables set in this file. It is preset to a default configuration, listening to one particular chain. Attestors can listen to multiple chains simultaneously. See an example [here](./observer/.env.template).

>! Important !
>
> When using Ethereum, you must provide an API key for Infura as an environment variable.
> (Option for listening to other providers/own nodes to be added later).

> You can provide your own PRIVATE_KEY too, but if omitted, the attestor will generate one for you. Take good care of this key.

You can set the environment variables and start the service in one go using the following format:
```sh
$ INFURA_API_KEY=[your-infura-api-key] PRIVATE_KEY=[your-private-key] docker compose up
```

### Option 2. Build and run locally

- Set up a `.env` file in the `./observer` folder according to the template file
- Run the `build_and_start.sh` script

```bash
STORAGE_API_ENDPOINT="https://dev-oracle.dlc.link/storage-api" FUNDED_URL="https://stacks-observer-mocknet.herokuapp.com/funded" BTC_RPC_URL="electrs-btc2.dlc.link:18443/wallet/alice" RPC_USER="devnet2" RPC_PASS="devnet2" ORACLE_URL="https://dev-oracle.dlc.link/oracle" STORAGE_API_ENABLED=true RUST_LOG=warn,dlc_protocol_wallet=info cargo run
```

## Key management (WIP)


* Note, you can change the RUST_LOG to RUST_LOG=warn,dlc_protocol_wallet=debug for more debugging of this app's functioning.

Docker Compose example:

- go into the docker folder and create a .env file like this (you can make a duplicate of the .env.template file and rename it to .env):

```
CONTRACT_CLEANUP_ENABLED: "false",
ELECTRUM_API_URL: "https://dev-oracle.dlc.link/electrs/",
BITCOIN_NETWORK: "regtest",
DOCKER_PUBLIC_REGISTRY_PREFIX=public.ecr.aws/dlc-link/,
FUNDED_URL: "https://stacks-observer-mocknet.herokuapp.com/funded",
ORACLE_URL: "https://dev-oracle.dlc.link/oracle",
RUST_LOG: "warn,dlc_protocol_wallet=debug",
RUST_BACKTRACE: "full",
STORAGE_API_ENABLED: "true",
STORAGE_API_ENDPOINT: "https://dev-oracle.dlc.link/storage-api",
```

Then run:

```
docker-compose up -d
```

If you run into an authentication error when pulling down the docker image like this:

`Error response from daemon: pull access denied for public.ecr.aws/dlc-link/dlc-protocol-wallet, repository does not exist or may require 'docker login': denied: Your authorization token has expired. Reauthenticate and try again`

Run the authentication command like this:
`aws ecr-public get-login-password --region us-east-1 | docker login --username AWS --password-stdin public.ecr.aws`

as per this article: https://docs.aws.amazon.com/AmazonECR/latest/public/public-registries.html#public-registry-auth

## API documentation:

See [wallet.yaml](docs/wallet.yaml) - the content can be copied to [swagger editor](https://editor.swagger.io/)

## API Description

### List all oracle events (announcements)
