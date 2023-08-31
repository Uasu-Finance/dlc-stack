FROM cimg/rust:1.71.0-node AS it-build

USER circleci

# Install necessary dependencies and wasm-pack, and clean up
RUN sudo apt-get update && \
  sudo apt-get install -y curl jq bash build-essential clang && \
  cargo install wasm-pack && \
  sudo apt-get clean

COPY ./clients/ /app/clients
COPY ./dlc-link-manager/ /app/dlc-link-manager
COPY ./dlc-memory-storage-provider/ /app/dlc-memory-storage-provider
COPY ./js-interface-wallet/ /app/js-interface-wallet
COPY ./esplora-async-blockchain-provider/ /app/esplora-async-blockchain-provider
COPY ./wasm-wallet/ /app/wasm-wallet

COPY ./it/ /app/it

WORKDIR /app

RUN rustup override set nightly-2023-03-31 && \
  sudo chown -R circleci:circleci /app && \
  wasm-pack build --target bundler /app/wasm-wallet

WORKDIR /app/it

RUN jq '. + {type: "module", main: "dlc-wasm-wallet.js"} | del(.module)' ../wasm-wallet/pkg/package.json > temp.json && \
  mv temp.json ../wasm-wallet/pkg/package.json

RUN npm install && \
  sudo npm ci wasm-wallet


FROM node:16-alpine AS it

COPY --from=it-build /app/it/index.js /app/it/index.js
COPY --from=it-build /app/it/config.js /app/it/config.js
COPY --from=it-build /app/it/package.json /app/it/package.json
COPY --from=it-build /app/it/node_modules /app/it/node_modules
COPY --from=it-build /app/wasm-wallet/pkg /app/wasm-wallet/pkg

WORKDIR /app/it

CMD npm start
