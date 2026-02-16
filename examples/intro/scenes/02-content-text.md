---
template: content-text
duration: auto
props:
  heading: "How It Works"
  body: "Write scenes in Markdown with YAML frontmatter. Pick a template, set your props, and vidgen renders HTML in headless Chromium — frame by frame — then encodes to MP4 with FFmpeg."
---

vidgen's pipeline is straightforward: your markdown scenes become HTML via Handlebars templates, then Chromium screenshots each frame, and FFmpeg stitches them into a video.
