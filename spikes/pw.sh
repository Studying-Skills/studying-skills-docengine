#!/bin/sh
# Run a command inside the Playwright image (Chromium, Firefox, WebKit with their system libraries).
# usage: spikes/pw.sh <dir under spikes> <command...>
D=$1; shift
exec docker run --rm --ipc=host \
  -v /root/projects/studying-skills/studying-skills-frontend:/root/projects/studying-skills/studying-skills-frontend:ro \
  -v /root/projects/studying-skills/studying-skills-docengine:/root/projects/studying-skills/studying-skills-docengine \
  -w /root/projects/studying-skills/studying-skills-docengine/spikes/$D \
  mcr.microsoft.com/playwright:v1.65.0-noble "$@"
