# Darker Sim — static Rust/WASM site. Stage 1 builds it with trunk, stage 2 serves it with nginx on port 80.
FROM rust:1-slim-bookworm AS build
ARG TRUNK_VERSION=0.22.1
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && rustup target add wasm32-unknown-unknown \
 && case "${TARGETARCH:-amd64}" in arm64) A=aarch64 ;; *) A=x86_64 ;; esac \
 && curl -fsSL "https://github.com/trunk-rs/trunk/releases/download/v${TRUNK_VERSION}/trunk-${A}-unknown-linux-gnu.tar.gz" \
    | tar -xz -C /usr/local/bin \
 && trunk --version
WORKDIR /src
COPY . .
# This repository ships no game data. It must be generated locally (scripts/update.ps1) before building.
RUN if [ ! -f app/data/game.json ] || [ ! -d data/icons ]; then \
      echo "ERROR: app/data/game.json or data/icons/ is missing. Generate the game data with scripts/update.ps1 first." >&2; \
      exit 1; \
    fi
# index.html copies ../../../app/data/game.json, ../../../data/archetypes.json and ../../../data/icons into dist/
RUN cd sim/crates/dad-web && trunk build --release

FROM nginx:1.27-alpine
COPY deploy/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /src/sim/crates/dad-web/dist /usr/share/nginx/html
EXPOSE 80
HEALTHCHECK --interval=30s --timeout=3s CMD wget -qO- http://127.0.0.1/ >/dev/null || exit 1
