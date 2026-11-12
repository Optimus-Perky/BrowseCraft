# BrowseCraft sync server. Build: docker build -t browsecraft-server server/
FROM rust:1-slim-trixie AS build
# MIT Kerberos (GSSAPI) for single sign-on.
RUN apt-get update && apt-get install -y --no-install-recommends libkrb5-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml clippy.toml ./
COPY src ./src
RUN cargo build --release --locked 2>/dev/null || cargo build --release

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends libgssapi-krb5-2 ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --home /data browsecraft && mkdir -p /data && chown browsecraft /data
COPY --from=build /src/target/release/browsecraft-server /usr/local/bin/browsecraft-server
USER browsecraft
ENV BROWSECRAFT_DB=/data/browsecraft.db BROWSECRAFT_LISTEN=0.0.0.0:8295 KRB5RCACHEDIR=/data
VOLUME /data
EXPOSE 8295
ENTRYPOINT ["browsecraft-server"]
CMD ["serve"]
