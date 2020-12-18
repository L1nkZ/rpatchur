set -ex

# Incorporate TARGET env var to the build and test process
if [[ $TARGET != *-musl ]]; then
  cargo build --target "$TARGET" --verbose
  cargo test --target "$TARGET" --verbose
else
  # Build with musl in a Docker container
  docker build -t build-"$PROJECT_NAME" -f docker/Dockerfile-musl .
  chmod -R 777 "$TRAVIS_BUILD_DIR"
  docker run -v "$TRAVIS_BUILD_DIR":/home/rust/src build-"$PROJECT_NAME" --verbose
fi

