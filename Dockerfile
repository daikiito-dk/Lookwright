# Lookwright — headless spatial-UI test runner (SPEC.md §10).
# No GPU, no display, no headset: everything renders on the CPU.
#
# Build:  docker build -t lookwright .
# Run:    docker run --rm -v "$PWD:/work" lookwright \
#           test --config /work/examples/tests/walk_and_look.test.json

FROM rust:1.93-slim AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo build --release -p lookwright-cli

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/lookwright /usr/local/bin/lookwright
COPY examples /usr/local/share/lookwright/examples
WORKDIR /work
ENTRYPOINT ["lookwright"]
CMD ["--help"]
