FROM rust:1.82-slim AS build
WORKDIR /app
COPY Cargo.toml .
COPY src ./src
RUN cargo build --release
FROM debian:bookworm-slim
COPY --from=build /app/target/release/aurelia-api /usr/local/bin/aurelia-api
EXPOSE 8080
CMD ["aurelia-api"]