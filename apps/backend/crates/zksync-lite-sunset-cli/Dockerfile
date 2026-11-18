####################################################################################################
## Foundry builds the claim-distributor ABIs that the core crate consumes via `sol!`.
####################################################################################################
FROM ghcr.io/foundry-rs/foundry:latest AS claims_abi

USER root
WORKDIR /workspace
COPY contracts/zksync-lite-migration-claims ./contracts/zksync-lite-migration-claims
WORKDIR /workspace/contracts/zksync-lite-migration-claims
RUN rm -rf out && forge build

####################################################################################################
## Build the CLI binary.
####################################################################################################
FROM rust:alpine3.23 AS build

RUN apk update && \
    apk upgrade --no-cache && \
    apk add --no-cache \
      build-base \
      musl-dev \
      clang \
      lld \
      cmake \
      pkgconfig \
      openssl-dev \
      openssl-libs-static \
      ca-certificates

WORKDIR /workspace
COPY apps/backend ./apps/backend
RUN mkdir -p /workspace/contracts/zksync-lite-migration-claims
COPY --from=claims_abi /workspace/contracts/zksync-lite-migration-claims/out /workspace/contracts/zksync-lite-migration-claims/out

WORKDIR /workspace/apps/backend

ENV OPENSSL_STATIC=1
ENV RUSTFLAGS="-C strip=symbols"
RUN cargo build -p zksync-lite-sunset-cli --release

####################################################################################################
## Runtime image: static CLI binary + snapshot CSVs baked in at /data.
####################################################################################################
FROM alpine:3.23

RUN apk update && \
    apk upgrade --no-cache && \
    apk add --no-cache ca-certificates tzdata && \
    update-ca-certificates && \
    echo "hosts: files dns" > /etc/nsswitch.conf

ENV USER=importer
ENV UID=10001
RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    "${USER}"

COPY --from=build /workspace/apps/backend/target/release/zksync-lite-sunset-cli /bin/zksync-lite-sunset-cli
COPY --chown=${UID}:${UID} data /data

USER importer:importer
WORKDIR /tmp

# DATABASE_URL is read from the environment.
# `claims-import` imports the snapshot CSVs and materializes claims in a single pass.
# --truncate wipes snapshot_accounts/tokens/balances before re-import.
# --reset-runtime-state truncates migration_requests/claim_requests/user_token_states.
# Extra flags (e.g. --snapshot-label foo, --chunk-size 10000) can be appended as container args.
ENTRYPOINT ["/bin/sh", "-c", "exec /bin/zksync-lite-sunset-cli claims-import --database-url \"$DATABASE_URL\" --data-dir /data --truncate --reset-runtime-state \"$@\"", "--"]
