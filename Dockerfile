# Appliance image: native supervisor, cloud profile, checksummed RVFA host.
# Build: docker build -t ruflo-appliance .
# Run:   docker run --rm ruflo-appliance

FROM rust:1.87-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release --locked -p ruflo --no-default-features

FROM debian:bookworm-slim
RUN useradd --create-home --uid 10001 ruflo \
    && mkdir -p /etc/ruflo /home/ruflo/workspace/.claude-flow
COPY --from=build /src/target/release/ruflo /usr/local/bin/ruflo
COPY config/appliance/cloud.yaml /etc/ruflo/cloud.yaml
COPY config/appliance/cloud.yaml /home/ruflo/workspace/.claude-flow/config.yaml
WORKDIR /home/ruflo/workspace
RUN ruflo appliance build -o /etc/ruflo/ruflo.rvfa --profile cloud \
    && chown -R ruflo:ruflo /etc/ruflo /home/ruflo
USER ruflo
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s \
  CMD ruflo daemon status || exit 1
ENTRYPOINT ["ruflo", "daemon", "start", "--foreground", "--ttl", "0"]
