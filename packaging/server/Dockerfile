FROM node:22-bookworm-slim AS web
WORKDIR /src
COPY package*.json ./
RUN npm ci
COPY . .
RUN npm run build

FROM rust:1-bookworm AS builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev libdbus-1-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
RUN cargo build --locked --release -p harness-cli

FROM debian:bookworm-slim AS cpu
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 libdbus-1-3 libgomp1 && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --home-dir /var/lib/harness harness
COPY --from=builder /src/target/release/harness /usr/lib/harness/harness
COPY --from=web /src/build /usr/lib/harness/web
USER 10001:10001
ENV HOME=/var/lib/harness
VOLUME ["/var/lib/harness"]
EXPOSE 11500
ENTRYPOINT ["/usr/lib/harness/harness", "--data-dir", "/var/lib/harness", "serve", "--port", "11500"]

FROM cpu AS vulkan
USER root
RUN apt-get update && apt-get install -y --no-install-recommends libvulkan1 mesa-vulkan-drivers && rm -rf /var/lib/apt/lists/*
USER 10001:10001
