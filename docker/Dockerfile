FROM rust:1.70 AS build

COPY . /app/dlc-stack/
WORKDIR /app/dlc-stack/

RUN rustup override set nightly-2023-03-31
RUN cargo build --release

FROM debian:bullseye-slim AS dlc-link-stack
RUN apt-get update && apt-get install -y openssl libpq-dev ca-certificates && apt-get clean

COPY --from=build /app/dlc-stack/target/release/storage-api /app/storage-api
COPY --from=build /app/dlc-stack/target/release/dlc-protocol-wallet /app/dlc-protocol-wallet

WORKDIR /app
ENTRYPOINT ["/app/dlc-protocol-wallet"]
