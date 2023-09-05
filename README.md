# dlc-stack

![build workflow](https://github.com/dlc-link/dlc-stack/actions/workflows/docker-build.yml/badge.svg)

Modules that work together to provide a seamless lending experience for DLC.Link.

## Modules

### Attestor

The `attestor` module is providing a numeric oracle implementation for Bitcoin, creating  announcements / attestations, triggered by events on a blockchain. It is primarily a NodeJs application, which is using a wasm module for the actual oracle implementation. See its [README](./attestor/README.md) for more details.

### Router Wallet
The `wallet` module is responsible for communicating with the `dlc-manager` contracts on the smart-chain side, and the Attestor Layer. It provides an API for creating and managing DLCs. See its [README](./wallet/README.md) for more details.

### WASM Wallet
The `wasm-wallet` module is a compact solution to be used in browser wallets. See its [README](./wasm-wallet/README.md) for more details.

### Storage-API

The `storage-api` module provides an API for hiding storage operations. Currently, it is implemented to work with Postgres as the underlying storage engine. See its [README](./storage/README.md) for more details.

### Clients

The `clients` module provides re-usable clients for the attestor / wallet / storage-api.

### IT (WIP)

The `it` module provides basic integration tests using BDD (Behavior-Driven Development) with Cucumber.

## Build

```bash
# build all modules (rust)
make build
# build using docker
make docker-build
# build & run docker compose (see docker-compose.yml)
make docker-start
```

## Known Build Errors

On Apple silicon (M1, M2) there are some known difficulties building the diesel package. This may materialize as something like the following:
```bash
  = note: ld: warning: directory not found for option '-L/usr/local/lib/postgresql@14'
          ld: library not found for -lpq
          clang: error: linker command failed with exit code 1 (use -v to see invocation)
```
Running the build in the following way may solve this problem. `RUSTFLAGS='-L /opt/homebrew/opt/libpq/lib' cargo build`

The following thread discusses in more details.
https://github.com/diesel-rs/diesel/issues/2605



## License

APM 2.0
