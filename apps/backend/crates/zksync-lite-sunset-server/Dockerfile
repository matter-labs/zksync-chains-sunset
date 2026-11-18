####################################################################################################
## Build
####################################################################################################
FROM ghcr.io/foundry-rs/foundry:latest AS claims_abi

USER root
WORKDIR /workspace
COPY contracts/zksync-lite-migration-claims ./contracts/zksync-lite-migration-claims
WORKDIR /workspace/contracts/zksync-lite-migration-claims
RUN rm -rf out && forge build

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
RUN cargo build -p zksync-lite-sunset-server --release


####################################################################################################
## Runtime files
####################################################################################################
FROM alpine:3.23 AS files

RUN apk update && \
    apk upgrade --no-cache && \
    apk add --no-cache ca-certificates tzdata

RUN update-ca-certificates
RUN echo "hosts: files dns" > /etc/nsswitch.conf

ENV USER=server
ENV UID=10001
RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    "${USER}"


####################################################################################################
## Final image
####################################################################################################
FROM files

COPY --from=build /workspace/apps/backend/target/release/zksync-lite-sunset-server /bin/zksync-lite-sunset-server

USER server:server
WORKDIR /tmp

EXPOSE 3000
ENV ZKSYNC_LITE_SUNSET__SERVER__HOST=0.0.0.0
ENV ZKSYNC_LITE_SUNSET__SERVER__PORT=3000

ENTRYPOINT ["/bin/zksync-lite-sunset-server"]
