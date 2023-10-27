FROM rustlang/rust:nightly-bullseye AS build

COPY ./wallet/src /app/dlc-protocol-wallet/src
COPY ./wallet/Cargo.toml /app/dlc-protocol-wallet/Cargo.toml
COPY ./dlc-link-manager /app/dlc-link-manager/
COPY ./clients /app/clients/
COPY ./attestor-client /app/attestor-client/
COPY ./dlc-wallet /app/dlc-wallet/
COPY ./esplora-async-blockchain-provider-js-wallet /app/esplora-async-blockchain-provider-js-wallet/
COPY ./esplora-async-blockchain-provider-router-wallet /app/esplora-async-blockchain-provider-router-wallet/
COPY ./js-interface-wallet /app/js-interface-wallet/

WORKDIR /app/dlc-protocol-wallet/
RUN cargo build --release --bin dlc-protocol-wallet

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y openssl libpq-dev ca-certificates curl && apt-get clean
COPY --from=build /app/dlc-protocol-wallet/target/release/dlc-protocol-wallet /app/dlc-protocol-wallet
WORKDIR /app
ENTRYPOINT ["/app/dlc-protocol-wallet"]
