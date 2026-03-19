#!/bin/bash
# Generate video clips needed by this example project.
# Requires: cargo build --features clipper,youtube
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
VIDGEN="${VIDGEN:-cargo run --features clipper,youtube --}"

echo "Generating clips for video-clips example..."

# Website scroll capture (requires --features clipper)
$VIDGEN clip web "https://crates.io/crates/vidgen" \
  -p "$SCRIPT_DIR" -d 5 --scroll-speed 150 --fps 24 -o "crates-io-vidgen"

# YouTube clip (requires --features youtube)
$VIDGEN clip youtube "https://www.youtube.com/watch?v=dQw4w9WgXcQ" \
  -p "$SCRIPT_DIR" --from 0 --to 5 -o "yt-sample"

echo "Done! Now render with: vidgen render $SCRIPT_DIR"
