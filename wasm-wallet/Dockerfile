FROM rust:1.65 AS build

COPY . /app/dlc-protocol-wallet/
WORKDIR /app/dlc-protocol-wallet/

RUN cargo build --release --bin dlc-protocol-wallet

FROM debian:buster-slim AS dlc-link-stack
RUN apt-get update && apt-get install -y openssl libpq-dev ca-certificates && apt-get clean
COPY --from=build /app/dlc-protocol-wallet/target/release/dlc-protocol-wallet /app/dlc-protocol-wallet
WORKDIR /app
ENTRYPOINT ["/app/dlc-protocol-wallet"]
