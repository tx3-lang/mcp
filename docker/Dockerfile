FROM rust:1.87 AS build

WORKDIR /app

RUN apt update 
RUN apt install -y build-essential pkg-config libssl-dev cmake 

COPY ./Cargo.toml ./Cargo.toml
COPY ./Cargo.lock ./Cargo.lock
COPY ./schema.graphql ./schema.graphql
COPY ./build.rs ./build.rs
COPY ./src ./src

WORKDIR /app

RUN cargo build --release

FROM rust:1.87-slim

COPY --from=build /app/target/release/sse /app/bin/sse

WORKDIR /app/bin

CMD ["./sse"]