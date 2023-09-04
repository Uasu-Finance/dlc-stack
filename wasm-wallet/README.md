# wasm-wallet
This library provides DLC functionality, as a javascript library, for bitcoin wallets that want to support accepting DLCs.

This library will be built and deployed periodically by the DLC.Link team to an npm package, for wallets to use. To learn more about integrating this package into your bitcoin wallet, please see the following documentation:

https://docs.dlc.link/architecture/installation-and-setup/bitcoin-wallets

## How to build
As a wasm build of a rust project, you can build this project with the following command:
```bash
AR=/opt/homebrew/opt/llvm/bin/llvm-ar CC=/opt/homebrew/opt/llvm/bin/clang wasm-pack build
```
