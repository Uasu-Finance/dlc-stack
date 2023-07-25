FROM rust:1.70 AS build

COPY . /app/dlc-stack/
WORKDIR /app/dlc-stack/

RUN rustup override set nightly-2023-03-31
RUN cargo build --release

FROM debian:buster-slim AS dlc-link-stack
RUN apt-get update && apt-get install -y openssl libpq-dev ca-certificates && apt-get clean
# COPY --from=build /app/dlc-stack/target/release/sibyls /app/sibyls
COPY --from=build /app/dlc-stack/target/release/storage-api /app/storage-api
COPY --from=build /app/dlc-stack/target/release/dlc-protocol-wallet /app/dlc-protocol-wallet
# COPY --from=build /app/dlc-stack/target/release/oracle-discovery /app/oracle-discovery

WORKDIR /app
ENTRYPOINT ["/app/sibyls"]
