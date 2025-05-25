# Use the official Rust image as a parent image
FROM rust:1.86 as builder

# Set the working directory in the container
WORKDIR /usr/src/oxxy

# for paho
RUN apt-get update && apt-get install -y cmake

# Copy the entire project
COPY . .

# Build the project
RUN cargo build --release

# Start a new stage with a minimal image
FROM debian:bullseye-slim

# Install OpenSSL - required for many Rust applications
RUN apt-get update && apt-get install -y openssl ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the binary from the builder stage
COPY --from=builder /usr/src/oxxy/target/release/loxxy /usr/local/bin/loxxy
COPY --from=builder /usr/src/oxxy/target/release/moxxy /usr/local/bin/moxxy
COPY --from=builder /usr/src/oxxy/target/release/roxxy /usr/local/bin/roxxy
COPY --from=builder /usr/src/oxxy/target/release/toxxy /usr/local/bin/toxxy

# Set the startup command to run your binary
CMD ["loxxy"]
