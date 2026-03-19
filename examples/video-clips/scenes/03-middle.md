---
template: sequence
duration: auto
sub_scenes:
  - template: content-text
    duration: 3
    props:
      heading: "What is vidgen?"
      body: "An AI-agent-first video production CLI written in Rust."
  - video_source: "@assets/clips/crates-io-vidgen.mp4"
    duration: auto
    source_volume: 0.0
  - template: content-text
    duration: 3
    props:
      heading: "Key Features"
      body: "HTML/CSS scenes, headless Chromium, FFmpeg encoding, MCP server."
---

Vidgen lets you create videos from markdown files. Here's the crate page on crates dot io where you can find the latest version. The project supports multiple templates, voiceover synthesis, and video clip integration.
