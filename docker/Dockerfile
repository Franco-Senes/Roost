# Imagen oficial del emulador de Roost (multi-stage)
# Incluye ffmpeg del repositorio Debian (GPLv2+/LGPLv2.1+); ver docs/emulator.md.

FROM node:20-alpine AS web-builder
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1-bookworm AS rust-builder
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY --from=web-builder /src/web/dist ./web/dist
RUN cargo build --release --bin roost

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /opt/roost
COPY --from=rust-builder /src/target/release/roost /usr/local/bin/roost
COPY apps/ /opt/roost/apps/

ENV ROOST_ADDR=0.0.0.0:8420
ENV ROOST_APPS_DIR=/opt/roost/apps
ENV ROOST_DATA_DIR=/var/lib/roost
ENV ROOST_DEMO=1

EXPOSE 8420
VOLUME ["/var/lib/roost"]

CMD ["/usr/local/bin/roost"]
