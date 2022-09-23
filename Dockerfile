# We use a multi-stage build.
# The builder stage creates a PostgreSQL database, runs the migrations on it and builds the app.
# The final stage just copies the resulting binary into a new container together with the static files.

#  We create from postgres and install the rest on top
FROM postgres:14-bullseye as builder

# Install Rust, Cargo and sqlx-cli
# Here, we make sure that sqlx-cli is on the same minor version as sqlx manually.
ENV PATH=/root/.cargo/bin:$PATH
RUN apt-get update && \
    apt-get install -y curl build-essential pkg-config openssl libssl-dev && \
    sh -c "curl https://sh.rustup.rs -sSf | sh -s -- --default-toolchain stable -y" && \
    cargo install sqlx-cli@^0.5 --no-default-features --features rustls,postgres

# Configure database
ENV POSTGRES_USER=audition_dates \
    POSTGRES_PASSWORD=audition_dates \
    DATABASE_URL=postgres://audition_dates:audition_dates@127.0.0.1/audition_dates

# Build app
WORKDIR /app
COPY . .
RUN ["/bin/bash", "-c", "{ /usr/local/bin/docker-entrypoint.sh postgres & } && { until pg_isready --host=127.0.0.1; do sleep 1; done } && cargo sqlx migrate run && cargo install --path . && cargo clean"]

# Copy binary to new image for smaller image size
FROM debian:bullseye
WORKDIR /app
COPY --from=builder /root/.cargo/bin/audition-dates .
COPY static static
COPY templates templates
COPY templates-mail templates-mail
CMD ["./audition-dates"]
