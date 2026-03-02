FROM node:20-slim AS webbuilder
WORKDIR /web
COPY nabaos-web/package.json nabaos-web/package-lock.json ./
RUN npm ci
COPY nabaos-web/ ./
RUN npm run build

FROM rust:1-slim-bookworm AS builder
WORKDIR /build
# Install build deps
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY --from=webbuilder /web/dist/ nabaos-web/dist/
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
# SECURITY: Create non-root user to minimize container escape risk
RUN groupadd -r nyaya && useradd -r -g nyaya -d /home/nyaya -s /sbin/nologin nyaya
COPY --from=builder /build/target/release/nabaos /usr/local/bin/
COPY config/ /etc/nabaos/config/
RUN mkdir -p /data /models && chown -R nyaya:nyaya /data /models
ENV NABA_DATA_DIR=/data
ENV NABA_MODEL_PATH=/models
VOLUME ["/data", "/models"]
USER nyaya
ENTRYPOINT ["nabaos"]
CMD ["start"]
